//! SPEC: P1-VIEW-012 — IPC surface for the recents list.
//!
//! These three commands are the only callers that know where
//! `recents.json` lives. They resolve `app_data_dir()` via the Tauri
//! `AppHandle`, take the recents file lock from `AppState`, and
//! delegate the actual list/file work to `settings::recents` (which is
//! `AppHandle`-free and unit-tested directly).
//!
//! Each command returns the post-mutation list so the frontend renders
//! exactly what the backend persisted — the backend is the single
//! source of truth for cap/dedup ordering.

use std::path::PathBuf;

use tauri::{AppHandle, Manager, State};

use crate::error::CommandError;
use crate::settings::recents;
use crate::AppState;

/// Resolve `<app_data_dir>/recents.json`. No hardcoded paths — the
/// directory is whatever Tauri reports for this platform.
fn recents_path(app: &AppHandle) -> Result<PathBuf, CommandError> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| CommandError::Internal(format!("app_data_dir unavailable: {e}")))?;
    Ok(dir.join("recents.json"))
}

/// SPEC: P1-VIEW-012 — read the persisted recents list (most-recent
/// first). Missing/corrupt file → empty list.
#[tauri::command]
pub async fn recents_list(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<String>, CommandError> {
    let path = recents_path(&app)?;
    let _guard = state
        .recents_lock
        .lock()
        .map_err(|e| CommandError::Internal(format!("recents lock poisoned: {e}")))?;
    Ok(recents::load(&path))
}

/// SPEC: P1-VIEW-012 — record `path` as the most-recent file
/// (dedup + move-to-front + cap at 20). Returns the new list.
#[tauri::command]
pub async fn recents_push(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<Vec<String>, CommandError> {
    let file = recents_path(&app)?;
    let _guard = state
        .recents_lock
        .lock()
        .map_err(|e| CommandError::Internal(format!("recents lock poisoned: {e}")))?;
    let mut list = recents::load(&file);
    recents::push_front(&mut list, path);
    recents::save(&file, &list)?;
    Ok(list)
}

/// SPEC: P1-VIEW-012 — clear the list (UI **and** disk). Returns `[]`.
#[tauri::command]
pub async fn recents_clear(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<String>, CommandError> {
    let file = recents_path(&app)?;
    let _guard = state
        .recents_lock
        .lock()
        .map_err(|e| CommandError::Internal(format!("recents lock poisoned: {e}")))?;
    recents::save(&file, &[])?;
    Ok(Vec::new())
}
