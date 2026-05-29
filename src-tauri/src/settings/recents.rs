//! SPEC: P1-VIEW-012 — the last 20 opened files, clearable, persisted
//! to `<app_data_dir>/recents.json`.
//!
//! This module is split into two layers:
//!
//! - **Pure list logic** (`push_front`, `MAX_RECENTS`) — no IO, fully
//!   unit-testable.
//! - **Disk IO** (`load`, `save`) against an explicit `&Path`, so tests
//!   can point it at a temp file and the command layer can point it at
//!   the Tauri-resolved `app_data_dir`. Neither layer knows about
//!   `AppHandle`; that lives in `commands/recents.rs`.
//!
//! Defensive posture: a missing or corrupt `recents.json` reads as an
//! empty list rather than erroring — recents are a convenience, never a
//! reason to block the start screen. Writes are atomic (temp file +
//! rename) so an interrupted write can't truncate the real file.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::CommandError;

/// Maximum number of remembered files. Oldest entries beyond this are
/// pruned on every push.
pub const MAX_RECENTS: usize = 20;

/// On-disk shape. `version` is carried for forward-compat: a future
/// format bump can branch on it instead of silently mis-parsing.
#[derive(Debug, Default, Serialize, Deserialize)]
struct RecentsFile {
    version: u32,
    paths: Vec<String>,
}

const CURRENT_VERSION: u32 = 1;

/// Insert `path` at the front of `list`, removing any existing equal
/// entry first (so re-opening a file moves it to the top rather than
/// duplicating it), then truncate to [`MAX_RECENTS`].
///
/// Pure: no IO, no clones beyond what the `Vec` needs.
pub fn push_front(list: &mut Vec<String>, path: String) {
    list.retain(|p| p != &path);
    list.insert(0, path);
    list.truncate(MAX_RECENTS);
}

/// Read the recents list from `file`. A missing file yields an empty
/// list (first run); a corrupt or wrong-version file also yields an
/// empty list rather than an error — see the module docs.
#[must_use]
pub fn load(file: &Path) -> Vec<String> {
    let Ok(bytes) = std::fs::read(file) else {
        return Vec::new();
    };
    match serde_json::from_slice::<RecentsFile>(&bytes) {
        Ok(parsed) if parsed.version == CURRENT_VERSION => {
            let mut paths = parsed.paths;
            // Defend against a hand-edited file that exceeds the cap.
            paths.truncate(MAX_RECENTS);
            paths
        }
        _ => Vec::new(),
    }
}

/// Atomically persist `list` to `file`. Creates the parent directory if
/// it does not yet exist (first run, before Tauri has touched
/// `app_data_dir`). Writes to a sibling temp file then renames, so a
/// crash mid-write leaves the previous `recents.json` intact.
pub fn save(file: &Path, list: &[String]) -> Result<(), CommandError> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let payload = RecentsFile {
        version: CURRENT_VERSION,
        paths: list.iter().take(MAX_RECENTS).cloned().collect(),
    };
    let json = serde_json::to_vec_pretty(&payload)
        .map_err(|e| CommandError::Internal(format!("serialize recents: {e}")))?;

    // Unique temp sibling so two concurrent saves can't clobber each
    // other's temp file before the rename. The caller already holds the
    // recents mutex, but the unique suffix is cheap insurance.
    let tmp = file.with_extension(format!("json.tmp-{}", uuid::Uuid::new_v4()));
    std::fs::write(&tmp, &json)?;
    std::fs::rename(&tmp, file)?;
    Ok(())
}
