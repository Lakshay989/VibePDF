use std::path::PathBuf;

use serde::Serialize;
use tauri::State;

use crate::error::CommandError;
use crate::pdf::actor::DocumentActorHandle;
use crate::pdf::document::open_document_metadata;
use crate::AppState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedDocument {
    pub id: String,
    pub path: String,
    pub name: String,
    pub page_count: u32,
}

/// SPEC: P1-VIEW-001 — open a PDF by absolute path.
///
/// Bootstrap version: opens the document via PDFium, reads page count and
/// metadata, spawns the document actor, and returns the handle id to the
/// frontend. Streaming reads, mmap, and progress events come in a Phase 1
/// follow-up.
#[tauri::command]
pub async fn pdf_open(
    state: State<'_, AppState>,
    path: String,
) -> Result<OpenedDocument, CommandError> {
    let path_buf = PathBuf::from(&path);
    if !path_buf.is_file() {
        return Err(CommandError::NotFound(path.clone()));
    }

    let meta = open_document_metadata(&path_buf)?;
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(path_buf.clone());

    {
        let mut guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        guard.insert(id, handle);
    }

    Ok(OpenedDocument {
        id: id.to_string(),
        name: path_buf
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.clone()),
        path,
        page_count: meta.page_count,
    })
}

/// Diagnostic: returns the PDFium version string. Used by the smoke test
/// to prove the native library actually loaded.
#[tauri::command]
pub async fn pdfium_version() -> Result<String, CommandError> {
    Ok(crate::pdf::document::pdfium_version_string())
}
