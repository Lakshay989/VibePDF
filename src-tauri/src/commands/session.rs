//! SPEC: P1-VIEW-011 — IPC surface for session restore.
//!
//! Resolves `<app_data_dir>/session.json`, takes `AppState.session_lock`,
//! and delegates to `settings::session` (which is `AppHandle`-free and
//! unit-tested directly).

use std::path::PathBuf;

use tauri::{AppHandle, Manager, State};

use crate::error::CommandError;
use crate::settings::session::{self, Session};
use crate::AppState;

fn session_path(app: &AppHandle) -> Result<PathBuf, CommandError> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| CommandError::Internal(format!("app_data_dir unavailable: {e}")))?;
    Ok(dir.join("session.json"))
}

/// SPEC: P1-VIEW-011 — read the persisted session (open tabs + active).
/// Missing/corrupt → empty session.
#[tauri::command]
pub async fn session_load(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Session, CommandError> {
    let path = session_path(&app)?;
    let _guard = state
        .session_lock
        .lock()
        .map_err(|e| CommandError::Internal(format!("session lock poisoned: {e}")))?;
    Ok(session::load(&path))
}

/// SPEC: P1-VIEW-011 — persist the current open tabs and active tab.
/// Called by the frontend whenever the tab set changes.
#[tauri::command]
pub async fn session_save(
    app: AppHandle,
    state: State<'_, AppState>,
    open: Vec<String>,
    active: Option<String>,
) -> Result<(), CommandError> {
    let path = session_path(&app)?;
    let _guard = state
        .session_lock
        .lock()
        .map_err(|e| CommandError::Internal(format!("session lock poisoned: {e}")))?;
    session::save(&path, &Session { open, active })
}
