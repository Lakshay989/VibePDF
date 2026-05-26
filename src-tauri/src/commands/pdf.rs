use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::error::CommandError;
use crate::pdf::actor::DocumentActorHandle;
use crate::AppState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedDocument {
    pub id: String,
    pub path: String,
    pub name: String,
    pub page_count: u32,
    pub title: Option<String>,
    pub author: Option<String>,
    pub pdf_version: Option<String>,
}

/// SPEC: P1-VIEW-001 — open a PDF by absolute path.
/// SPEC: P1-VIEW-002 — invalid files return a typed `PdfError`, never panic.
///
/// The document is opened *inside* the per-document actor thread, not in
/// this async handler. We wait on the actor's ready-channel for either
/// the cached metadata or a typed error.
#[tauri::command]
pub async fn pdf_open(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<OpenedDocument, CommandError> {
    let path_buf = PathBuf::from(&path);
    if !path_buf.is_file() {
        return Err(CommandError::NotFound(path.clone()));
    }

    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(Some(app), id, path_buf.clone(), None)?;
    let meta = handle.metadata().clone();

    {
        let mut guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        guard.insert(id, handle);
    }

    Ok(OpenedDocument {
        id: id.to_string(),
        name: path_buf.file_name().map_or_else(
            || path.clone(),
            |s| s.to_string_lossy().into_owned(),
        ),
        path,
        page_count: meta.page_count,
        title: meta.title,
        author: meta.author,
        pdf_version: meta.pdf_version,
    })
}

/// Drops the actor for `id`, which closes its mailbox and ends the
/// worker thread. Idempotent: closing an already-closed id returns Ok.
#[tauri::command]
pub async fn pdf_close(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), CommandError> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;

    let removed = {
        let mut guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        guard.remove(&uuid)
    };

    if let Some(handle) = removed {
        handle.close();
        // Sender drops at end of scope, mailbox closes, worker exits.
        drop(handle);
    }
    Ok(())
}

/// Diagnostic: returns the `PDFium` version string. Used by the smoke test
/// to prove the native library actually loaded.
#[tauri::command]
pub async fn pdfium_version() -> Result<String, CommandError> {
    Ok(crate::pdf::document::pdfium_version_string())
}
