//! Auto-save + crash recovery (P2.A2).
//!
//! SPEC: infrastructure — `docs/04_ARCHITECTURE.md` § "Saving and
//! auto-save". Every [`AUTOSAVE_INTERVAL`] the tick thread pokes each
//! document actor; a *dirty* actor writes a recovery copy to
//! `<app_data_dir>/autosave/<id>.pdf` plus a `<id>.json` sidecar
//! recording the original path. On startup the frontend lists these via
//! the `recovery_list` command and offers to reopen them.
//!
//! Auto-save is best-effort and **never touches the user's original
//! file**. Nothing makes a document dirty in P2.A2 (page edits are the
//! P2.B* steps), so the live loop is dormant; these functions are tested
//! directly, and the end-to-end crash demo lands with P2.B2.

use std::path::{Path, PathBuf};
use std::time::Duration;

use pdfium_render::prelude::PdfDocument;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::error::CommandError;
use crate::AppState;

/// How often the tick thread pokes actors to autosave if dirty.
pub const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(30);

/// Sidecar written next to each autosaved PDF, recording where it came
/// from so recovery can label it (and, later, write back to the original).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AutosaveSidecar {
    document_id: String,
    original_path: String,
    saved_at: u64,
}

/// One recoverable document, surfaced to the frontend. Wire type for the
/// `recovery_list` command.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryEntry {
    pub document_id: String,
    pub original_path: String,
    pub autosave_path: String,
    pub saved_at: u64,
}

/// Resolve `<app_data_dir>/autosave`, creating it if missing. No
/// hardcoded paths — the location comes from Tauri's path resolver.
pub fn autosave_dir(app: &AppHandle) -> Result<PathBuf, CommandError> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| CommandError::Internal(format!("app_data_dir unavailable: {e}")))?
        .join("autosave");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Write a recovery copy of `doc` to `<dir>/<document_id>.pdf` plus a
/// JSON sidecar. Both writes are atomic (temp + rename) so a crash
/// mid-write cannot leave a torn recovery file.
///
/// SPEC: `docs/04` § "Saving and auto-save". Uses the same
/// `save_to_bytes` serialization as the explicit-save path (P2.A1).
pub fn write_autosave(
    doc: &PdfDocument<'_>,
    dir: &Path,
    document_id: &str,
    original_path: &str,
) -> Result<PathBuf, CommandError> {
    std::fs::create_dir_all(dir)?;

    let pdf_path = dir.join(format!("{document_id}.pdf"));
    let pdf_tmp = dir.join(format!("{document_id}.pdf.tmp"));
    let bytes = {
        let _guard = crate::pdf::document::pdfium_lock()?;
        doc.save_to_bytes().map_err(CommandError::from)?
    };
    std::fs::write(&pdf_tmp, &bytes)?;
    std::fs::rename(&pdf_tmp, &pdf_path)?;

    let sidecar = AutosaveSidecar {
        document_id: document_id.to_string(),
        original_path: original_path.to_string(),
        saved_at: unix_now(),
    };
    let json = serde_json::to_vec_pretty(&sidecar)
        .map_err(|e| CommandError::Internal(format!("sidecar serialize: {e}")))?;
    let json_path = dir.join(format!("{document_id}.json"));
    let json_tmp = dir.join(format!("{document_id}.json.tmp"));
    std::fs::write(&json_tmp, &json)?;
    std::fs::rename(&json_tmp, &json_path)?;

    Ok(pdf_path)
}

/// List recoverable documents in `dir`. A missing directory yields an
/// empty list; malformed or orphaned entries (unparseable sidecar, or a
/// sidecar whose `.pdf` is gone) are skipped rather than failing the scan.
pub fn scan_autosaves(dir: &Path) -> Result<Vec<RecoveryEntry>, CommandError> {
    let mut out = Vec::new();
    let read = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e.into()),
    };
    for entry in read {
        let path = entry?.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let Ok(raw) = std::fs::read(&path) else {
            continue;
        };
        let Ok(sidecar) = serde_json::from_slice::<AutosaveSidecar>(&raw) else {
            continue;
        };
        let pdf_path = dir.join(format!("{}.pdf", sidecar.document_id));
        if !pdf_path.is_file() {
            continue; // orphaned sidecar
        }
        out.push(RecoveryEntry {
            document_id: sidecar.document_id,
            original_path: sidecar.original_path,
            autosave_path: pdf_path.to_string_lossy().into_owned(),
            saved_at: sidecar.saved_at,
        });
    }
    Ok(out)
}

/// Remove a document's autosave copy + sidecar — after a real save, a
/// clean close, or the user recovering/discarding it. Missing files are
/// not an error.
pub fn discard_autosave(dir: &Path, document_id: &str) -> Result<(), CommandError> {
    for ext in ["pdf", "json"] {
        let p = dir.join(format!("{document_id}.{ext}"));
        match std::fs::remove_file(&p) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

/// Spawn the background autosave tick: every [`AUTOSAVE_INTERVAL`], poke
/// each open actor to autosave if dirty.
///
/// A dedicated std thread (so we need no `tokio` `time` feature); the poke
/// is fire-and-forget — the actor writes its copy and logs. The thread
/// runs for the process lifetime.
pub fn spawn_autosave_tick(app: AppHandle) {
    let spawned = std::thread::Builder::new()
        .name("autosave-tick".into())
        .spawn(move || loop {
            std::thread::sleep(AUTOSAVE_INTERVAL);
            let state = app.state::<AppState>();
            let Ok(guard) = state.actors.lock() else {
                continue;
            };
            for handle in guard.values() {
                handle.poke_autosave();
            }
        });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "failed to spawn autosave-tick thread");
    }
}
