use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::error::CommandError;
use crate::pdf::actor::DocumentActorHandle;
use crate::pdf::render::{ImageFormat, RenderedPage};
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
/// SPEC: P1-VIEW-003 — encrypted files surface as `PasswordRequired`
/// (rather than the generic `PdfError`) so the frontend can mount the
/// prompt dialog. The `password` arg is `None` for the first attempt and
/// `Some(...)` for every retry; `PDFium` does not distinguish "no password
/// supplied" from "wrong password", so both flow through the same arm.
///
/// The document is opened *inside* the per-document actor thread, not in
/// this async handler. We wait on the actor's ready-channel for either
/// the cached metadata or a typed error.
#[tauri::command]
pub async fn pdf_open(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    password: Option<String>,
) -> Result<OpenedDocument, CommandError> {
    let path_buf = PathBuf::from(&path);
    if !path_buf.is_file() {
        return Err(CommandError::NotFound(path.clone()));
    }

    let id = uuid::Uuid::new_v4();
    let handle =
        DocumentActorHandle::spawn(Some(app), id, path_buf.clone(), password).map_err(|e| {
            // The From<PdfiumError> impl in error.rs leaves the
            // PasswordRequired payload empty (the conversion site
            // doesn't know the path). Enrich it here so the frontend
            // dialog can label which file it's prompting for.
            match e {
                CommandError::PasswordRequired(_) => CommandError::PasswordRequired(path.clone()),
                other => other,
            }
        })?;
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

/// SPEC: P1-VIEW-008 + NFR-PERF-003 — render a page to either PNG or
/// raw RGBA8 bytes. The thumbnail sidebar (D1), full-page viewer
/// (future), export-to-image (P3), and render-failure log (E2) all
/// route through this single command.
#[tauri::command]
pub async fn pdf_render_page(
    state: State<'_, AppState>,
    id: String,
    page: u32,
    dpi: f32,
    format: ImageFormat,
) -> Result<RenderedPage, CommandError> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;

    // Send the request while holding the map lock, then drop the
    // lock before awaiting the reply — otherwise the actor map is
    // held across an `.await`, which blocks every other command on
    // every other document for the duration of the render.
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.render_page_request(page, dpi, format)?
    };

    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// Diagnostic: returns the `PDFium` version string. Used by the smoke test
/// to prove the native library actually loaded.
#[tauri::command]
pub async fn pdfium_version() -> Result<String, CommandError> {
    Ok(crate::pdf::document::pdfium_version_string())
}
