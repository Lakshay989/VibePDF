//! P6.A1 — IPC surface for the signature library.
//!
//! These four commands are the only callers that know where the library lives.
//! They resolve `app_data_dir()` via the Tauri `AppHandle`, take the library
//! lock from `AppState`, and delegate to `settings::signatures` (which is
//! `AppHandle`-free and unit-tested directly) — the same shape as
//! `commands/recents.rs`.
//!
//! The lock matters more here than for recents: `add` is a read-modify-write of
//! the index *plus* a blob write, so two concurrent adds without it could lose
//! an entry.

use std::path::PathBuf;

use tauri::{AppHandle, Manager, State};

use crate::error::CommandError;
use crate::settings::signatures::{self, SignatureEntry, SignatureKind};
use crate::AppState;

/// Resolve `<app_data_dir>/signatures/`. No hardcoded paths — the directory is
/// whatever Tauri reports for this platform.
///
/// `pub(crate)` for `commands::pdf::pdf_place_signature` (P6.A5a), which resolves
/// a library id to bytes before handing them to the document actor. Keeping one
/// definition means placement can never disagree with the library about where
/// the blobs are.
pub(crate) fn library_dir(app: &AppHandle) -> Result<PathBuf, CommandError> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| CommandError::Internal(format!("app_data_dir unavailable: {e}")))?;
    Ok(dir.join("signatures"))
}

/// Milliseconds since the Unix epoch. A clock before 1970 yields 0 rather than
/// failing the write — the timestamp is a sort key, not a security claim.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// P6.A1 — the stored signatures, newest first. Missing/corrupt index → empty.
#[tauri::command]
pub async fn signatures_list(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<SignatureEntry>, CommandError> {
    let dir = library_dir(&app)?;
    let _guard = state
        .signatures_lock
        .lock()
        .map_err(|e| CommandError::Internal(format!("signatures lock poisoned: {e}")))?;
    Ok(signatures::load(&dir))
}

/// P6.A1 — store `bytes` (PNG) as a new signature of `kind`. Returns the new
/// entry; the oldest are pruned past the cap.
#[tauri::command]
pub async fn signatures_add(
    app: AppHandle,
    state: State<'_, AppState>,
    kind: String,
    bytes: Vec<u8>,
) -> Result<SignatureEntry, CommandError> {
    let kind = SignatureKind::parse(&kind)?;
    let dir = library_dir(&app)?;
    let _guard = state
        .signatures_lock
        .lock()
        .map_err(|e| CommandError::Internal(format!("signatures lock poisoned: {e}")))?;
    signatures::add(&dir, kind, &bytes, now_ms())
}

/// P6.A1 — delete a signature and its blob. Unknown id is a no-op.
#[tauri::command]
pub async fn signatures_remove(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), CommandError> {
    let dir = library_dir(&app)?;
    let _guard = state
        .signatures_lock
        .lock()
        .map_err(|e| CommandError::Internal(format!("signatures lock poisoned: {e}")))?;
    signatures::remove(&dir, &id)
}

/// P6.A1 — the PNG bytes for `id`, for rendering a preview or placing it.
#[tauri::command]
pub async fn signatures_bytes(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<u8>, CommandError> {
    let dir = library_dir(&app)?;
    let _guard = state
        .signatures_lock
        .lock()
        .map_err(|e| CommandError::Internal(format!("signatures lock poisoned: {e}")))?;
    signatures::bytes(&dir, &id)
}
