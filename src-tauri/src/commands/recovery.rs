use tauri::AppHandle;

use crate::error::CommandError;
use crate::pdf::autosave::{self, RecoveryEntry};

/// SPEC: P2.A2 — list documents that have an unsaved-changes recovery
/// copy on disk, so the frontend can offer to reopen them at startup.
#[tauri::command]
pub async fn recovery_list(app: AppHandle) -> Result<Vec<RecoveryEntry>, CommandError> {
    let dir = autosave::autosave_dir(&app)?;
    autosave::scan_autosaves(&dir)
}

/// SPEC: P2.A2 — drop a document's recovery copy after the user has
/// recovered or declined it.
#[tauri::command]
pub async fn recovery_discard(app: AppHandle, id: String) -> Result<(), CommandError> {
    let dir = autosave::autosave_dir(&app)?;
    autosave::discard_autosave(&dir, &id)
}
