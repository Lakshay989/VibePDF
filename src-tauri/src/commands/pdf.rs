use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::error::CommandError;
use crate::pdf::actor::DocumentActorHandle;
use crate::pdf::clean::{CleanOptions, CleanOutcome};
use crate::pdf::cos::{AnnotationInfo, FreeTextData, MeasureCalibration, NoteData, TextBoxInfo};
use crate::pdf::font_resolver::FontReport;
use crate::pdf::form::{
    ButtonField, ChoiceField, FieldProperties, FormField, FormSummary, NewFieldKind, PageField,
};
use crate::pdf::form_data::ExportFormat;
use crate::pdf::form_import::ImportOutcome;
use crate::pdf::image_extract::ImageInfo;
use crate::pdf::text_extract::TextRun;
use crate::pdf::document::{open_document_metadata, SaveOutcome};
use crate::pdf::merge::merge_documents;
use crate::pdf::render::{ImageFormat, RenderedPage};
use crate::pdf::split::{SplitMode, SplitOutcome};
use crate::pdf::background::BackgroundKind;
use crate::pdf::undo::HistoryState;
use crate::pdf::watermark::WatermarkKind;
use crate::security::encrypt::DocumentPermissions;
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

/// SPEC: P2-PAGE-001 — rotate `pages` (0-based indices) by `degrees`
/// (a multiple of 90°). Persisted as `PDFium` `/Rotate`, recorded on the
/// undo stack, and marks the document dirty. Returns the new history
/// availability so the caller can update the Undo/Redo button state.
#[tauri::command]
pub async fn pdf_rotate_pages(
    state: State<'_, AppState>,
    id: String,
    pages: Vec<i32>,
    degrees: i32,
) -> Result<HistoryState, CommandError> {
    if degrees % 90 != 0 {
        return Err(CommandError::InvalidInput(format!(
            "rotation must be a multiple of 90°, got {degrees}"
        )));
    }
    if pages.is_empty() {
        return Err(CommandError::InvalidInput("no pages specified".into()));
    }
    let quarter_turns = degrees / 90;

    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;

    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.rotate_pages_request(pages, quarter_turns)?
    };

    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P2-PAGE-010 — resize `pages` (0-based) to `width` × `height` points,
/// scaling content to fit. `preserve_aspect` scales uniformly and centres;
/// otherwise content is stretched. Undoable, marks dirty; returns the new
/// history availability.
#[tauri::command]
pub async fn pdf_resize_pages(
    state: State<'_, AppState>,
    id: String,
    pages: Vec<i32>,
    width: f32,
    height: f32,
    preserve_aspect: bool,
) -> Result<HistoryState, CommandError> {
    if pages.is_empty() {
        return Err(CommandError::InvalidInput("no pages specified".into()));
    }
    if !(width.is_finite() && height.is_finite()) || width <= 0.0 || height <= 0.0 {
        return Err(CommandError::InvalidInput(format!(
            "resize dimensions must be positive and finite, got {width}×{height}"
        )));
    }
    // Guard against absurd sizes (PDF's practical max page side is 200" = 14400pt).
    if width > 14_400.0 || height > 14_400.0 {
        return Err(CommandError::InvalidInput(format!(
            "resize dimensions exceed the 14400pt limit, got {width}×{height}"
        )));
    }

    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;

    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.resize_pages_request(pages, width, height, preserve_aspect)?
    };

    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P3-ANN-001 — add a text-markup annotation over `quads` (each
/// `[x1..y4]` in PDF points) on `page` (0-based). `subtype` is one of
/// `highlight` / `underline` / `strikethrough` / `squiggly`; `color` is
/// `#rrggbb`. Undoable, marks dirty; returns the new history availability.
#[tauri::command]
pub async fn pdf_add_text_markup(
    state: State<'_, AppState>,
    id: String,
    page: i32,
    subtype: String,
    quads: Vec<[f32; 8]>,
    color: String,
    opacity: f32,
) -> Result<HistoryState, CommandError> {
    if quads.is_empty() {
        return Err(CommandError::InvalidInput("no quads specified".into()));
    }
    if page < 0 {
        return Err(CommandError::InvalidInput(format!("negative page index: {page}")));
    }

    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;

    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.add_text_markup_request(page, subtype, quads, color, opacity)?
    };

    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P3-ANN-001 — remove all text-markup annotations from the document.
/// Undoable, marks dirty; returns the new history availability.
#[tauri::command]
pub async fn pdf_clear_text_markup(
    state: State<'_, AppState>,
    id: String,
) -> Result<HistoryState, CommandError> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;

    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.clear_text_markup_request()?
    };

    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P3-ANN-002 — add a sticky note (`/Text` annotation) at `(x, y)` on
/// `page` (0-based). `note_id` becomes its `/NM` (the update/delete handle).
/// Undoable; returns the new history availability.
#[tauri::command]
#[allow(clippy::too_many_arguments)] // flat args are the IPC contract.
pub async fn pdf_add_text_note(
    state: State<'_, AppState>,
    id: String,
    note_id: String,
    page: i32,
    x: f32,
    y: f32,
    content: String,
    author: String,
) -> Result<HistoryState, CommandError> {
    if page < 0 {
        return Err(CommandError::InvalidInput(format!("negative page index: {page}")));
    }
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.add_note_request(note_id, page, x, y, content, author)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P3-ANN-009 — reply to the annotation `parent_id` (its sidebar handle).
/// Persists a `/Text` linked via `/IRT`. Undoable; runs on the actor.
#[tauri::command]
pub async fn pdf_add_reply(
    state: State<'_, AppState>,
    id: String,
    parent_id: String,
    author: String,
    content: String,
) -> Result<HistoryState, CommandError> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.add_reply_request(parent_id, author, content)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P3-ANN-002 — update the body of the note with `/NM == note_id`.
#[tauri::command]
pub async fn pdf_update_text_note(
    state: State<'_, AppState>,
    id: String,
    note_id: String,
    content: String,
) -> Result<HistoryState, CommandError> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.update_note_request(note_id, content)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P3-ANN-002 — delete the annotation with `/NM == note_id`.
#[tauri::command]
pub async fn pdf_delete_annotation(
    state: State<'_, AppState>,
    id: String,
    note_id: String,
) -> Result<HistoryState, CommandError> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.delete_annotation_request(note_id)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P3-ANN-002 (re-openable) — read every sticky note (`/Text` annotation)
/// out of the open document so the frontend can project them into its note
/// overlay on open and after undo/redo. Read-only; no edit, no history entry.
#[tauri::command]
pub async fn pdf_read_text_notes(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<NoteData>, CommandError> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.read_notes_request()?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P3-ANN-008 — read every supported annotation for the sidebar list.
/// Read-only; no edit, no history entry.
#[tauri::command]
pub async fn pdf_read_annotations(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<AnnotationInfo>, CommandError> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.read_annotations_request()?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P3-ANN-010 — export every annotation in the document to an XFDF file at
/// `path`. Read-only on the source; returns the count of annotations written.
#[tauri::command]
pub async fn pdf_export_annotations(
    state: State<'_, AppState>,
    id: String,
    path: String,
) -> Result<usize, CommandError> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.export_annotations_request(PathBuf::from(path))?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P3-ANN-010 — import annotations from the XFDF file at `path`, added as
/// one undoable edit. The file is read here (the actor stays byte-pure); the
/// add itself runs on the actor.
#[tauri::command]
pub async fn pdf_import_annotations(
    state: State<'_, AppState>,
    id: String,
    path: String,
) -> Result<HistoryState, CommandError> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let xfdf = std::fs::read_to_string(&path)
        .map_err(|e| CommandError::InvalidInput(format!("cannot read {path}: {e}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.import_xfdf_request(xfdf)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P3-ANN-011 — flatten every `/AP`-bearing annotation into the page
/// content streams. Undoable in-session; permanent once saved + reopened.
#[tauri::command]
pub async fn pdf_flatten_annotations(
    state: State<'_, AppState>,
    id: String,
) -> Result<HistoryState, CommandError> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.flatten_annotations_request()?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P3-ANN-013 — read a free-text annotation's editable text + style by
/// `/NM`, so the in-place editor can open pre-filled. Read-only.
#[tauri::command]
pub async fn pdf_read_free_text(
    state: State<'_, AppState>,
    id: String,
    nm: String,
) -> Result<Option<FreeTextData>, CommandError> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.read_free_text_request(nm)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P3-ANN-013 — update a free-text annotation (by `/NM`) in place: new
/// text + style. Undoable; runs on the actor.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn pdf_update_free_text(
    state: State<'_, AppState>,
    id: String,
    nm: String,
    text: String,
    font_family: String,
    font_size: f32,
    color: String,
    bold: bool,
    italic: bool,
    underline: bool,
) -> Result<HistoryState, CommandError> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.update_free_text_request(nm, text, font_family, font_size, color, bold, italic, underline)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P3-ANN-003 — add a free-text box at `rect` (`[x0,y0,x1,y1]` PDF pts) on
/// `page` (0-based) with `text` in a base-14 font. Undoable; runs on the actor.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn pdf_add_free_text(
    state: State<'_, AppState>,
    id: String,
    page: i32,
    rect: [f32; 4],
    text: String,
    font_family: String,
    font_size: f32,
    color: String,
    bold: bool,
    italic: bool,
    underline: bool,
) -> Result<HistoryState, CommandError> {
    if page < 0 {
        return Err(CommandError::InvalidInput(format!("negative page index: {page}")));
    }
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.add_free_text_request(page, rect, text, font_family, font_size, color, bold, italic, underline)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P4-EDIT-003 (P4.B2) — add a text box as **page content** (not an
/// annotation) at `rect` on `page` (0-based). Undoable; runs on the actor.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn pdf_add_text_box(
    state: State<'_, AppState>,
    id: String,
    page: i32,
    rect: [f32; 4],
    text: String,
    font_family: String,
    font_size: f32,
    color: String,
    bold: bool,
    italic: bool,
    underline: bool,
) -> Result<HistoryState, CommandError> {
    if page < 0 {
        return Err(CommandError::InvalidInput(format!("negative page index: {page}")));
    }
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.add_text_box_request(page, rect, text, font_family, font_size, color, bold, italic, underline)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P4-EDIT-003b — re-edit the Add-Text box `box_id` on `page` (0-based) in
/// place: replace its text + style, keeping its rectangle. Undoable; runs on the actor.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn pdf_update_text_box(
    state: State<'_, AppState>,
    id: String,
    page: i32,
    box_id: String,
    text: String,
    font_family: String,
    font_size: f32,
    color: String,
    bold: bool,
    italic: bool,
    underline: bool,
) -> Result<HistoryState, CommandError> {
    if page < 0 {
        return Err(CommandError::InvalidInput(format!("negative page index: {page}")));
    }
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.update_text_box_request(page, box_id, text, font_family, font_size, color, bold, italic, underline)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P4-EDIT-003b — read every re-editable Add-Text box on `page` (0-based)
/// for double-click re-edit hit-testing. Read-only; runs on the actor.
#[tauri::command]
pub async fn pdf_read_text_boxes(
    state: State<'_, AppState>,
    id: String,
    page: i32,
) -> Result<Vec<TextBoxInfo>, CommandError> {
    if page < 0 {
        return Err(CommandError::InvalidInput(format!("negative page index: {page}")));
    }
    let page = usize::try_from(page).unwrap_or(0);
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.read_text_boxes_request(page)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P5-FORM-001 — summarise the document's interactive form (terminal field
/// count + XFA flag) so the UI can surface a "Form mode" entry point. Read-only.
#[tauri::command]
pub async fn pdf_read_form_summary(
    state: State<'_, AppState>,
    id: String,
) -> Result<FormSummary, CommandError> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.read_form_summary_request()?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P5-FORM-002 — list the fillable text fields on `page` (0-based) with
/// geometry + current value, for the fill overlay. Read-only.
#[tauri::command]
pub async fn pdf_read_text_fields(
    state: State<'_, AppState>,
    id: String,
    page: i32,
) -> Result<Vec<FormField>, CommandError> {
    if page < 0 {
        return Err(CommandError::InvalidInput(format!("negative page index: {page}")));
    }
    let page = usize::try_from(page).unwrap_or(0);
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.read_text_fields_request(page)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P5-FORM-002 — set the text field `name` to `value` (truncated to the
/// field's `/MaxLen`). Undoable; runs on the actor.
#[tauri::command]
pub async fn pdf_fill_text_field(
    state: State<'_, AppState>,
    id: String,
    name: String,
    value: String,
) -> Result<HistoryState, CommandError> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.fill_text_field_request(name, value)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P5-FORM-003 — list the checkbox/radio widgets on `page` (0-based) with
/// geometry + state, for the button overlay. Read-only.
#[tauri::command]
pub async fn pdf_read_button_fields(
    state: State<'_, AppState>,
    id: String,
    page: i32,
) -> Result<Vec<ButtonField>, CommandError> {
    if page < 0 {
        return Err(CommandError::InvalidInput(format!("negative page index: {page}")));
    }
    let page = usize::try_from(page).unwrap_or(0);
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.read_button_fields_request(page)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P5-FORM-003 — toggle/select button field `name` to `on_state`.
/// Undoable; runs on the actor.
#[tauri::command]
pub async fn pdf_set_button_field(
    state: State<'_, AppState>,
    id: String,
    name: String,
    on_state: String,
    checked: bool,
) -> Result<HistoryState, CommandError> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.set_button_field_request(name, on_state, checked)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P5-FORM-004 — list the choice fields (combo/list) on `page` (0-based)
/// with options + selection, for the choice overlay. Read-only.
#[tauri::command]
pub async fn pdf_read_choice_fields(
    state: State<'_, AppState>,
    id: String,
    page: i32,
) -> Result<Vec<ChoiceField>, CommandError> {
    if page < 0 {
        return Err(CommandError::InvalidInput(format!("negative page index: {page}")));
    }
    let page = usize::try_from(page).unwrap_or(0);
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.read_choice_fields_request(page)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P5-FORM-004 — set choice field `name`'s selection to `values` (declared
/// export values). Undoable; runs on the actor.
#[tauri::command]
pub async fn pdf_set_choice_field(
    state: State<'_, AppState>,
    id: String,
    name: String,
    values: Vec<String>,
) -> Result<HistoryState, CommandError> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.set_choice_field_request(name, values)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P5-FORM-005 — drop the dynamic XFA layer of an XFA-only document (remove
/// `/XFA` + set `/NeedAppearances`) so its static content renders. Undoable.
#[tauri::command]
pub async fn pdf_strip_xfa(
    state: State<'_, AppState>,
    id: String,
) -> Result<HistoryState, CommandError> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.strip_xfa_request()?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P5-FORM-006 — create a text field on `page` (0-based) at `rect`
/// (`[x0,y0,x1,y1]` PDF points), configured with name/default/max-length/
/// multi-line/required. Undoable; runs on the actor.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn pdf_add_text_field(
    state: State<'_, AppState>,
    id: String,
    page: i32,
    rect: [f32; 4],
    name: String,
    default_value: String,
    max_len: Option<u32>,
    multiline: bool,
    required: bool,
) -> Result<HistoryState, CommandError> {
    if page < 0 {
        return Err(CommandError::InvalidInput(format!("negative page index: {page}")));
    }
    let page = usize::try_from(page).unwrap_or(0);
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.add_text_field_request(page, rect, name, default_value, max_len, multiline, required)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P5-FORM-007 — create a checkbox/radio/combo/list/signature/push-button
/// field on `page` (0-based) at `rect`. `kind` selects the field type; the other
/// params carry its config (`options`, `default_value`, `multi`, `required`,
/// `caption`). Undoable; runs on the actor.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn pdf_add_field(
    state: State<'_, AppState>,
    id: String,
    page: i32,
    rect: [f32; 4],
    name: String,
    kind: String,
    options: Vec<String>,
    default_value: String,
    multi: bool,
    required: bool,
    caption: String,
) -> Result<HistoryState, CommandError> {
    if page < 0 {
        return Err(CommandError::InvalidInput(format!("negative page index: {page}")));
    }
    let page = usize::try_from(page).unwrap_or(0);
    let kind = match kind.as_str() {
        "checkbox" => NewFieldKind::Checkbox { required },
        "radio" => NewFieldKind::RadioGroup { options },
        "combo" => NewFieldKind::Combo { options, default: default_value },
        "list" => NewFieldKind::ListBox { options, multi },
        "signature" => NewFieldKind::Signature,
        "pushbutton" => NewFieldKind::PushButton { caption },
        other => return Err(CommandError::InvalidInput(format!("unknown field kind: {other}"))),
    };
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.add_field_request(page, rect, name, kind)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P5-FORM-006b — list every form field on `page` (0-based), any kind, in
/// tab order, for the field-properties panel. Read-only.
#[tauri::command]
pub async fn pdf_read_page_fields(
    state: State<'_, AppState>,
    id: String,
    page: i32,
) -> Result<Vec<PageField>, CommandError> {
    if page < 0 {
        return Err(CommandError::InvalidInput(format!("negative page index: {page}")));
    }
    let page = usize::try_from(page).unwrap_or(0);
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.read_page_fields_request(page)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P5-FORM-006b — edit an existing field's properties. Every property is
/// optional: `null` leaves it untouched, `""`/`null` inside a `Some` clears it.
/// Undoable; runs on the actor.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn pdf_update_field_properties(
    state: State<'_, AppState>,
    id: String,
    name: String,
    new_name: Option<String>,
    default_value: Option<String>,
    max_len: Option<u32>,
    clear_max_len: bool,
    multiline: Option<bool>,
    required: Option<bool>,
    tooltip: Option<String>,
) -> Result<HistoryState, CommandError> {
    // `Option<Option<u32>>` can't round-trip through JSON, so the wire carries a
    // value plus an explicit clear flag: clear wins, then a value, else untouched.
    let max_len = if clear_max_len { Some(None) } else { max_len.map(Some) };
    let props = FieldProperties { new_name, default_value, max_len, multiline, required, tooltip };
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.update_field_properties_request(name, props)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P5-FORM-006c — set `page`'s tab order to `names`. Undoable.
#[tauri::command]
pub async fn pdf_set_tab_order(
    state: State<'_, AppState>,
    id: String,
    page: i32,
    names: Vec<String>,
) -> Result<HistoryState, CommandError> {
    if page < 0 {
        return Err(CommandError::InvalidInput(format!("negative page index: {page}")));
    }
    let page = usize::try_from(page).unwrap_or(0);
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.set_tab_order_request(page, names)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P5-FORM-006b — delete the form field `name` (and its widgets). Undoable.
#[tauri::command]
pub async fn pdf_delete_field(
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> Result<HistoryState, CommandError> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.delete_field_request(name)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P5-FORM-008 — export the document's form data (name, type, value) to
/// `dest` as FDF / XFDF / JSON / CSV. Read-only; replies with the field count.
#[tauri::command]
pub async fn pdf_export_form_data(
    state: State<'_, AppState>,
    id: String,
    format: String,
    dest: String,
) -> Result<usize, CommandError> {
    let format = ExportFormat::parse(&format)?;
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.export_form_data_request(format, std::path::PathBuf::from(dest))?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P5-FORM-009 — import form data from `src` (FDF / XFDF / JSON / CSV),
/// filling fields by name. Undoable; replies with the report (applied count,
/// unmatched names, type mismatches) plus the new history state.
#[tauri::command]
pub async fn pdf_import_form_data(
    state: State<'_, AppState>,
    id: String,
    format: String,
    src: String,
) -> Result<ImportOutcome, CommandError> {
    let format = ExportFormat::parse(&format)?;
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.import_form_data_request(format, std::path::PathBuf::from(src))?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P5-FORM-010 — flatten the interactive form: render each field's current
/// appearance into the page content and remove the field definitions. Undoable
/// in-session only; runs on the actor.
#[tauri::command]
pub async fn pdf_flatten_form(
    state: State<'_, AppState>,
    id: String,
) -> Result<HistoryState, CommandError> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.flatten_form_request()?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P6-SEC-012 (P6.D3) — remove everything `opts` names from the open
/// document: metadata, hidden text, comments, attachments, bookmarks, form
/// data, embedded files.
///
/// **In place, not on export**, unlike P6.C1/C2. Cleaning is an edit to the
/// document you are looking at — you want to see the comments disappear, and
/// you want Undo if you cleaned more than you meant to. The inverse is a
/// pre-clean byte snapshot, so undo works until the file is saved and reopened.
///
/// Returns the per-category counts: the page is unchanged by design, so without
/// them a clean is indistinguishable from having done nothing.
#[tauri::command]
pub async fn pdf_clean_document(
    state: State<'_, AppState>,
    id: String,
    options: CleanOptions,
) -> Result<CleanOutcome, CommandError> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.clean_document_request(options)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P4-EDIT-003b / P4-EDIT-004 — delete the Add-Text box `box_id` on `page`
/// (0-based). Undoable; runs on the actor.
#[tauri::command]
pub async fn pdf_delete_text_box(
    state: State<'_, AppState>,
    id: String,
    page: i32,
    box_id: String,
) -> Result<HistoryState, CommandError> {
    if page < 0 {
        return Err(CommandError::InvalidInput(format!("negative page index: {page}")));
    }
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.delete_text_box_request(page, box_id)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P4-EDIT-005 (P4.C1) — add an image (PNG or JPEG) as **page content** at
/// `rect` on `page` (0-based), aspect-fit. Reads the file, then runs on the actor.
#[tauri::command]
pub async fn pdf_add_image(
    state: State<'_, AppState>,
    id: String,
    page: i32,
    rect: [f32; 4],
    image_path: String,
) -> Result<HistoryState, CommandError> {
    if page < 0 {
        return Err(CommandError::InvalidInput(format!("negative page index: {page}")));
    }
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let image = std::fs::read(&image_path)
        .map_err(|e| CommandError::InvalidInput(format!("cannot read {image_path}: {e}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.add_image_request(page, rect, image)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P4-EDIT-007 (P4.C3) — add a `/Link` annotation over `rect` on `page`
/// (0-based). `kind` is `url` | `email` | `page` | `named`; `value` is the
/// matching target (URL / address / 0-based target-page index / dest name).
/// SPEC: P4-EDIT-007b — `style` is `invisible` | `box` | `underline` in `color`
/// (`#rrggbb`). Undoable; runs on the actor.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn pdf_add_link(
    state: State<'_, AppState>,
    id: String,
    page: i32,
    rect: [f32; 4],
    kind: String,
    value: String,
    style: String,
    color: String,
) -> Result<HistoryState, CommandError> {
    if page < 0 {
        return Err(CommandError::InvalidInput(format!("negative page index: {page}")));
    }
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.add_link_request(page, rect, kind, value, style, color)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P4-EDIT-009 (P4.D2) — stamp a **text** watermark on `pages` (0-based) at
/// `opacity` (0..1) + `rotation` degrees, on top or `behind` content. Undoable.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn pdf_add_text_watermark(
    state: State<'_, AppState>,
    id: String,
    pages: Vec<i32>,
    text: String,
    font_family: String,
    font_size: f32,
    color: String,
    opacity: f32,
    rotation: f32,
    behind: bool,
) -> Result<HistoryState, CommandError> {
    let kind = WatermarkKind::Text {
        text,
        font_family,
        size: font_size,
        color,
        bold: false,
        italic: false,
    };
    run_watermark(&state, &id, pages, kind, opacity, rotation, behind).await
}

/// SPEC: P4-EDIT-009 (P4.D2) — stamp an **image** watermark (PNG/JPEG at
/// `image_path`) on `pages`. Reads the file, then runs on the actor. Undoable.
#[tauri::command]
pub async fn pdf_add_image_watermark(
    state: State<'_, AppState>,
    id: String,
    pages: Vec<i32>,
    image_path: String,
    opacity: f32,
    rotation: f32,
    behind: bool,
) -> Result<HistoryState, CommandError> {
    let image = std::fs::read(&image_path)
        .map_err(|e| CommandError::InvalidInput(format!("cannot read {image_path}: {e}")))?;
    let kind = WatermarkKind::Image(image);
    run_watermark(&state, &id, pages, kind, opacity, rotation, behind).await
}

/// Shared tail for both watermark commands: resolve the actor + dispatch.
async fn run_watermark(
    state: &State<'_, AppState>,
    id: &str,
    pages: Vec<i32>,
    kind: WatermarkKind,
    opacity: f32,
    rotation: f32,
    behind: bool,
) -> Result<HistoryState, CommandError> {
    let uuid = uuid::Uuid::parse_str(id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.add_watermark_request(pages, kind, opacity, rotation, behind)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P4-EDIT-009 — remove every watermark this app added, from all pages.
/// Undoable; runs on the actor.
#[tauri::command]
pub async fn pdf_remove_watermarks(
    state: State<'_, AppState>,
    id: String,
) -> Result<HistoryState, CommandError> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.remove_watermarks_request()?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P4-EDIT-008 (P4.D1) — fill `pages` (0-based) behind their content with a
/// solid `color` (`#rrggbb`) at `opacity` (0..1). Undoable.
#[tauri::command]
pub async fn pdf_add_color_background(
    state: State<'_, AppState>,
    id: String,
    pages: Vec<i32>,
    color: String,
    opacity: f32,
) -> Result<HistoryState, CommandError> {
    run_background(&state, &id, pages, BackgroundKind::Color(color), opacity).await
}

/// SPEC: P4-EDIT-008 (P4.D1) — fill `pages` behind their content with an image
/// (PNG/JPEG at `image_path`), cover-fit. Reads the file, then runs on the actor.
#[tauri::command]
pub async fn pdf_add_image_background(
    state: State<'_, AppState>,
    id: String,
    pages: Vec<i32>,
    image_path: String,
    opacity: f32,
) -> Result<HistoryState, CommandError> {
    let image = std::fs::read(&image_path)
        .map_err(|e| CommandError::InvalidInput(format!("cannot read {image_path}: {e}")))?;
    run_background(&state, &id, pages, BackgroundKind::Image(image), opacity).await
}

/// SPEC: P4-EDIT-008 (P4.D1b) — fill `pages` behind their content with the
/// 0-based `source_page` of the PDF at `source_path`, imported as a Form
/// `XObject` (contain-fit). Reads the source file, then runs on the actor.
#[tauri::command]
pub async fn pdf_add_pdf_background(
    state: State<'_, AppState>,
    id: String,
    pages: Vec<i32>,
    source_path: String,
    source_page: i32,
    opacity: f32,
) -> Result<HistoryState, CommandError> {
    let source = std::fs::read(&source_path)
        .map_err(|e| CommandError::InvalidInput(format!("cannot read {source_path}: {e}")))?;
    let page = usize::try_from(source_page)
        .map_err(|_| CommandError::InvalidInput(format!("negative source page: {source_page}")))?;
    run_background(&state, &id, pages, BackgroundKind::PdfPage { source, page }, opacity).await
}

/// SPEC: P4-EDIT-010 (P4.D3) — draw left/center/right header or footer text on
/// `pages` (0-based). `position` is `header` | `footer`; `{n}`/`{total}`/`{date}`
/// are substituted per page (`date` is the caller's formatted today). Undoable.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn pdf_add_header_footer(
    state: State<'_, AppState>,
    id: String,
    pages: Vec<i32>,
    position: String,
    left: String,
    center: String,
    right: String,
    font_family: String,
    font_size: f32,
    color: String,
    margin: f32,
    date: String,
) -> Result<HistoryState, CommandError> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.add_header_footer_request(
            pages, position, left, center, right, font_family, font_size, color, margin, date,
        )?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P4-EDIT-011 (P4.D4) — stamp a page number in `format` (from `start`) in
/// the `position`/`align` margin of every page except the 0-based `exclude`d ones.
/// Undoable.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn pdf_add_page_numbers(
    state: State<'_, AppState>,
    id: String,
    exclude: Vec<i32>,
    position: String,
    align: String,
    format: String,
    start: i32,
    font_family: String,
    font_size: f32,
    color: String,
    margin: f32,
) -> Result<HistoryState, CommandError> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.add_page_numbers_request(
            exclude, position, align, format, start, font_family, font_size, color, margin,
        )?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P4-EDIT-012 (P4.D5) — stamp a Bates id (`{prefix}{padded seq}{suffix}`,
/// from `start`) in the `position`/`align` margin of every page. Undoable.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn pdf_add_bates(
    state: State<'_, AppState>,
    id: String,
    position: String,
    align: String,
    prefix: String,
    suffix: String,
    padding: u32,
    start: i32,
    font_family: String,
    font_size: f32,
    color: String,
    margin: f32,
) -> Result<HistoryState, CommandError> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.add_bates_request(
            position, align, prefix, suffix, padding, start, font_family, font_size, color, margin,
        )?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// Shared tail for both background commands: resolve the actor + dispatch.
async fn run_background(
    state: &State<'_, AppState>,
    id: &str,
    pages: Vec<i32>,
    kind: BackgroundKind,
    opacity: f32,
) -> Result<HistoryState, CommandError> {
    let uuid = uuid::Uuid::parse_str(id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.add_background_request(pages, kind, opacity)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P3-ANN-004 — add a shape annotation (`/Square` or `/Circle`) at `rect`
/// on `page` (0-based). Undoable; runs on the actor.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn pdf_add_shape(
    state: State<'_, AppState>,
    id: String,
    page: i32,
    kind: String,
    rect: [f32; 4],
    stroke_color: String,
    fill_color: Option<String>,
    opacity: f32,
    stroke_width: f32,
) -> Result<HistoryState, CommandError> {
    if page < 0 {
        return Err(CommandError::InvalidInput(format!("negative page index: {page}")));
    }
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.add_shape_request(page, kind, rect, stroke_color, fill_color, opacity, stroke_width)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P3-ANN-004 — add a line (or arrow) annotation from `(x1,y1)` to
/// `(x2,y2)` on `page` (0-based). Undoable; runs on the actor.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn pdf_add_line(
    state: State<'_, AppState>,
    id: String,
    page: i32,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    arrow: bool,
    stroke_color: String,
    opacity: f32,
    stroke_width: f32,
) -> Result<HistoryState, CommandError> {
    if page < 0 {
        return Err(CommandError::InvalidInput(format!("negative page index: {page}")));
    }
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.add_line_request(page, x1, y1, x2, y2, arrow, stroke_color, opacity, stroke_width)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P3-ANN-004 — add a polygon (`closed`) or polyline (`!closed`) through
/// `points` on `page` (0-based). Undoable; runs on the actor.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn pdf_add_polygon(
    state: State<'_, AppState>,
    id: String,
    page: i32,
    closed: bool,
    points: Vec<[f32; 2]>,
    stroke_color: String,
    fill_color: Option<String>,
    opacity: f32,
    stroke_width: f32,
) -> Result<HistoryState, CommandError> {
    if page < 0 {
        return Err(CommandError::InvalidInput(format!("negative page index: {page}")));
    }
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.add_polygon_request(page, closed, points, stroke_color, fill_color, opacity, stroke_width)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P3-ANN-005 — add a freehand `/Ink` annotation through `points`
/// (`[x, y, pressure]`, smoothed frontend-side) on `page` (0-based). Undoable;
/// runs on the actor.
#[tauri::command]
pub async fn pdf_add_ink(
    state: State<'_, AppState>,
    id: String,
    page: i32,
    points: Vec<[f32; 3]>,
    color: String,
    opacity: f32,
    base_width: f32,
) -> Result<HistoryState, CommandError> {
    if page < 0 {
        return Err(CommandError::InvalidInput(format!("negative page index: {page}")));
    }
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.add_ink_request(page, points, color, opacity, base_width)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P3-ANN-006 — add a `/Stamp` annotation bounded by `rect` (PDF points,
/// 0-based `page`), with the bold uppercase `text` label. Undoable; on the actor.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn pdf_add_stamp(
    state: State<'_, AppState>,
    id: String,
    page: i32,
    rect: [f32; 4],
    text: String,
    name: String,
    color: String,
    opacity: f32,
) -> Result<HistoryState, CommandError> {
    if page < 0 {
        return Err(CommandError::InvalidInput(format!("negative page index: {page}")));
    }
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.add_stamp_request(page, rect, text, name, color, opacity)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P3-ANN-006 (P3.C3b) — add an image `/Stamp` from the PNG at `image_path`,
/// placed aspect-correct around the click `(x, y)` at `height` points tall, with
/// an optional `text` label. The file is read here (the actor stays byte-pure);
/// the embed + write run on the actor. Undoable.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn pdf_add_image_stamp(
    state: State<'_, AppState>,
    id: String,
    page: i32,
    x: f32,
    y: f32,
    height: f32,
    image_path: String,
    text: Option<String>,
    opacity: f32,
) -> Result<HistoryState, CommandError> {
    if page < 0 {
        return Err(CommandError::InvalidInput(format!("negative page index: {page}")));
    }
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let image = std::fs::read(&image_path)
        .map_err(|e| CommandError::InvalidInput(format!("cannot read {image_path}: {e}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.add_image_stamp_request(page, x, y, height, image, text, opacity)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P6-SEC-007 (P6.C1) — write a password-protected copy of the open
/// document to `path`, with AES-256 encryption.
///
/// **Protect-on-export, not protect-in-place.** The open document is untouched:
/// this writes an encrypted copy and leaves the actor's state, undo history and
/// current password exactly as they were. Encrypting in place would mean the
/// actor holding a document whose open-password had silently changed, every
/// later render needing it, and undo having to restore the old one — three ways
/// to lock someone out of their own file, for no gain the spec line asks for.
///
/// The output is re-opened with the password before this returns, so a file
/// that cannot be opened is reported here rather than discovered later.
#[tauri::command]
pub async fn pdf_protect(
    state: State<'_, AppState>,
    id: String,
    path: String,
    user_password: Option<String>,
    owner_password: Option<String>,
    permissions: Option<DocumentPermissions>,
) -> Result<(), CommandError> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;

    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.get_bytes_request()?
    };
    let bytes = rx
        .await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))??;

    // Passwords stay in this frame. They are never logged, never put into a
    // `CommandError` message, and never reach the actor — which logs its path
    // on every message.
    let opts = crate::security::encrypt::EncryptOptions {
        user_password,
        owner_password,
        // SPEC: P6-SEC-009 — an omitted set restricts nothing, which is what a
        // caller that predates C3 means and what `Default` gives.
        permissions: permissions.unwrap_or_default(),
    };
    let encrypted = crate::security::encrypt::encrypt_document(&bytes, &opts)?;

    let out = PathBuf::from(&path);
    std::fs::write(&out, &encrypted)
        .map_err(|e| CommandError::Internal(format!("could not write {path}: {e}")))?;

    // The round-trip rule, with the password the file now needs. An owner-only
    // document opens with none, so try that first and fall back.
    let open_with = opts_open_password(&opts);
    if let Err(e) = crate::pdf::document::open_pdf(&out, open_with) {
        let _ = std::fs::remove_file(&out);
        return Err(CommandError::PdfError(format!(
            "the protected file could not be re-opened, so it was not kept: {e}"
        )));
    }
    Ok(())
}

/// SPEC: P6-SEC-008 (P6.C2) — write an unprotected copy of the open document.
///
/// The mirror of `pdf_protect`, and an export for the same reasons. `password`
/// is the document's owner password; see `security::decrypt` for why that is
/// what `lopdf` enforces for AES-256.
///
/// The output is re-opened **with no password** before this returns. That is
/// the whole assertion: a file that is still encrypted and a file that is not
/// look identical from here, and only a reader can tell them apart.
#[tauri::command]
pub async fn pdf_remove_protection(
    state: State<'_, AppState>,
    id: String,
    path: String,
    password: String,
) -> Result<(), CommandError> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;

    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.get_bytes_request()?
    };
    let bytes = rx
        .await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))??;

    let unlocked = crate::security::decrypt::remove_protection(&bytes, &password)?;

    let out = PathBuf::from(&path);
    std::fs::write(&out, &unlocked)
        .map_err(|e| CommandError::Internal(format!("could not write {path}: {e}")))?;

    if let Err(e) = crate::pdf::document::open_pdf(&out, None) {
        let _ = std::fs::remove_file(&out);
        return Err(CommandError::PdfError(format!(
            "the unlocked file still needs a password, so it was not kept: {e}"
        )));
    }
    Ok(())
}

/// Which password re-opens the file we just wrote: the user password when there
/// is one, otherwise none (an owner-only document opens freely).
fn opts_open_password(opts: &crate::security::encrypt::EncryptOptions) -> Option<&str> {
    opts.user_password.as_deref().filter(|p| !p.is_empty())
}

/// SPEC: P6-SEC-004 (P6.A5a) — place a stored signature on the page as a
/// `/Stamp`, aspect-correct around `(x, y)` at `height` points tall.
///
/// Takes a library **id**, not a path. Only the command layer knows where the
/// library lives (P6.A1), and shipping ~30 KB of PNG out to the frontend purely
/// to hand it straight back would cost 4× that as JSON for no benefit.
///
/// Reuses `ImageStampEdit` wholesale: the PNG's alpha becomes an `/SMask`, so a
/// transparent signature composites rather than arriving in a white box. It
/// reads back as kind `"stamp"` and inherits list/delete/undo.
///
/// The library lock is taken and dropped before the actor map is touched, so the
/// two locks are never held at the same time.
///
/// This is the stamp half of P6-SEC-004. The other half — writing into an
/// existing `/Sig` field as a PKCS#7 signature — needs certificate signing
/// (P6.B1) and is not implemented; the frontend declines that case rather than
/// stamping a picture over a signature field, which would look signed without
/// being signed.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn pdf_place_signature(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    page: i32,
    x: f32,
    y: f32,
    height: f32,
    signature_id: String,
    opacity: f32,
) -> Result<HistoryState, CommandError> {
    if page < 0 {
        return Err(CommandError::InvalidInput(format!("negative page index: {page}")));
    }
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;

    let png = {
        let dir = crate::commands::signatures::library_dir(&app)?;
        let _guard = state
            .signatures_lock
            .lock()
            .map_err(|e| CommandError::Internal(format!("signatures lock poisoned: {e}")))?;
        crate::settings::signatures::bytes(&dir, &signature_id)?
    };

    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.add_image_stamp_request(page, x, y, height, png, None, opacity)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P3-ANN-007 — add a measurement annotation (`kind` =
/// distance|perimeter|area) through `points` (PDF points, 0-based `page`). The
/// `label` is the pre-computed value (computed against the user's calibration).
/// Undoable; runs on the actor.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn pdf_add_measure(
    state: State<'_, AppState>,
    id: String,
    page: i32,
    kind: String,
    points: Vec<[f32; 2]>,
    color: String,
    label: String,
    opacity: f32,
    stroke_width: f32,
    units_per_point: f32,
    unit: String,
) -> Result<HistoryState, CommandError> {
    if page < 0 {
        return Err(CommandError::InvalidInput(format!("negative page index: {page}")));
    }
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.add_measure_request(
            page, kind, points, color, label, opacity, stroke_width, units_per_point, unit,
        )?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P3-ANN-007 (P3.C4b) — read the document's measurement calibration (from
/// the first `/Measure` dict) so the tool can re-seed itself on reopen. Read-only.
#[tauri::command]
pub async fn pdf_read_measure_calibration(
    state: State<'_, AppState>,
    id: String,
) -> Result<Option<MeasureCalibration>, CommandError> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.read_measure_calibration_request()?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P4-EDIT-001 (P4.A1) — extract every text run on `page` (0-based) for
/// click-to-edit hit-testing. Read-only; runs on the live document via the actor.
#[tauri::command]
pub async fn pdf_extract_text_runs(
    state: State<'_, AppState>,
    id: String,
    page: i32,
) -> Result<Vec<TextRun>, CommandError> {
    if page < 0 {
        return Err(CommandError::InvalidInput(format!("negative page index: {page}")));
    }
    let page = usize::try_from(page).unwrap_or(0);
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.read_text_runs_request(page)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P4-EDIT-002 (P4.A2) — resolve the open document's fonts against the
/// system, so the UI can warn once when an edit would substitute a missing
/// face. Read-only; runs on the live document via the actor.
#[tauri::command]
pub async fn pdf_read_font_report(
    state: State<'_, AppState>,
    id: String,
) -> Result<FontReport, CommandError> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.read_font_report_request()?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P4-EDIT-001 (P4.B1) — replace text run `run_index` on `page` (0-based,
/// A1 ordering) with `new_text`, preserving its font/size/colour/matrix. Undoable;
/// returns the new history availability.
#[tauri::command]
pub async fn pdf_replace_text_run(
    state: State<'_, AppState>,
    id: String,
    page: i32,
    run_index: i32,
    new_text: String,
) -> Result<HistoryState, CommandError> {
    if page < 0 {
        return Err(CommandError::InvalidInput(format!("negative page index: {page}")));
    }
    if run_index < 0 {
        return Err(CommandError::InvalidInput(format!("negative run index: {run_index}")));
    }
    let page = usize::try_from(page).unwrap_or(0);
    let run_index = usize::try_from(run_index).unwrap_or(0);
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.replace_text_run_request(page, run_index, new_text)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P4-EDIT-004 (P4.B3) — remove text run `run_index` on `page` (0-based, A1
/// ordering) from the page content stream. Undoable; returns history availability.
#[tauri::command]
pub async fn pdf_delete_text_run(
    state: State<'_, AppState>,
    id: String,
    page: i32,
    run_index: i32,
) -> Result<HistoryState, CommandError> {
    if page < 0 {
        return Err(CommandError::InvalidInput(format!("negative page index: {page}")));
    }
    if run_index < 0 {
        return Err(CommandError::InvalidInput(format!("negative run index: {run_index}")));
    }
    let page = usize::try_from(page).unwrap_or(0);
    let run_index = usize::try_from(run_index).unwrap_or(0);
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.delete_text_run_request(page, run_index)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P4-EDIT-006 (P4.C2) — locate the images on `page` (0-based) for
/// click-to-select. Read-only; runs on the live document via the actor.
#[tauri::command]
pub async fn pdf_extract_images(
    state: State<'_, AppState>,
    id: String,
    page: i32,
) -> Result<Vec<ImageInfo>, CommandError> {
    if page < 0 {
        return Err(CommandError::InvalidInput(format!("negative page index: {page}")));
    }
    let page = usize::try_from(page).unwrap_or(0);
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.read_images_request(page)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P4-EDIT-006 (P4.C2) — override image `index`'s placement matrix on `page`
/// (move/resize/rotate). Undoable; runs on the actor.
#[tauri::command]
pub async fn pdf_transform_image(
    state: State<'_, AppState>,
    id: String,
    page: i32,
    index: i32,
    matrix: [f32; 6],
) -> Result<HistoryState, CommandError> {
    if page < 0 || index < 0 {
        return Err(CommandError::InvalidInput(format!("negative page/index: {page}/{index}")));
    }
    let (page, index) = (usize::try_from(page).unwrap_or(0), usize::try_from(index).unwrap_or(0));
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.transform_image_request(page, index, matrix)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P4-EDIT-006 (P4.C2) — delete image `index` on `page`. Undoable; on the actor.
#[tauri::command]
pub async fn pdf_delete_image(
    state: State<'_, AppState>,
    id: String,
    page: i32,
    index: i32,
) -> Result<HistoryState, CommandError> {
    if page < 0 || index < 0 {
        return Err(CommandError::InvalidInput(format!("negative page/index: {page}/{index}")));
    }
    let (page, index) = (usize::try_from(page).unwrap_or(0), usize::try_from(index).unwrap_or(0));
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.delete_image_request(page, index)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P4-EDIT-006 (P4.C2b) — replace image `index`'s pixels on `page` with the
/// PNG/JPEG at `image_path`, preserving placement. Reads the file; on the actor.
#[tauri::command]
pub async fn pdf_replace_image(
    state: State<'_, AppState>,
    id: String,
    page: i32,
    index: i32,
    image_path: String,
) -> Result<HistoryState, CommandError> {
    if page < 0 || index < 0 {
        return Err(CommandError::InvalidInput(format!("negative page/index: {page}/{index}")));
    }
    let (page, index) = (usize::try_from(page).unwrap_or(0), usize::try_from(index).unwrap_or(0));
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;
    let image = std::fs::read(&image_path)
        .map_err(|e| CommandError::InvalidInput(format!("cannot read {image_path}: {e}")))?;
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.replace_image_request(page, index, image)?
    };
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P2-PAGE-003 — delete `pages` (0-based indices). `PDFium` renumbers
/// the page tree; the removed pages are preserved for undo. Marks the
/// document dirty; returns the new history availability.
#[tauri::command]
pub async fn pdf_delete_pages(
    state: State<'_, AppState>,
    id: String,
    pages: Vec<i32>,
) -> Result<HistoryState, CommandError> {
    if pages.is_empty() {
        return Err(CommandError::InvalidInput("no pages specified".into()));
    }

    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;

    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.delete_pages_request(pages)?
    };

    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P2-PAGE-004 — insert a blank page at `index` (0-based; `index ==
/// page count` appends). When both `width` and `height` (points) are given
/// they set the page size; otherwise it inherits the adjacent page's
/// dimensions. Undoable, marks the document dirty.
#[tauri::command]
pub async fn pdf_insert_blank_page(
    state: State<'_, AppState>,
    id: String,
    index: i32,
    width: Option<f32>,
    height: Option<f32>,
) -> Result<HistoryState, CommandError> {
    let size = match (width, height) {
        (Some(w), Some(h)) => Some((w, h)),
        _ => None,
    };

    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;

    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.insert_blank_page_request(index, size)?
    };

    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P2-PAGE-009 — crop `page` to a `CropBox` (left/bottom/right/top in
/// points). All four edges present → crop to that rectangle; all four
/// absent → reset to the `MediaBox`. Undoable, marks the document dirty.
#[tauri::command]
pub async fn pdf_crop_page(
    state: State<'_, AppState>,
    id: String,
    page: i32,
    left: Option<f32>,
    bottom: Option<f32>,
    right: Option<f32>,
    top: Option<f32>,
) -> Result<HistoryState, CommandError> {
    let rect = match (left, bottom, right, top) {
        (Some(l), Some(b), Some(r), Some(t)) => Some((l, b, r, t)),
        (None, None, None, None) => None,
        _ => {
            return Err(CommandError::InvalidInput(
                "crop requires all four edges or none (reset)".into(),
            ))
        }
    };

    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;

    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.crop_page_request(page, rect)?
    };

    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P2-PAGE-006 — extract `pages` (0-based) of the document into a new
/// PDF at `dest`. Read-only on the source; returns the write outcome.
#[tauri::command]
pub async fn pdf_extract_pages(
    state: State<'_, AppState>,
    id: String,
    pages: Vec<i32>,
    dest: String,
) -> Result<SaveOutcome, CommandError> {
    if pages.is_empty() {
        return Err(CommandError::InvalidInput("no pages specified".into()));
    }

    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;

    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.extract_pages_request(pages, std::path::PathBuf::from(dest))?
    };

    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// Serialize the live in-memory document to bytes, returned as **raw bytes**
/// via [`tauri::ipc::Response`] (an `ArrayBuffer` on the JS side), *not* a JSON
/// `number[]`. The edit-preview pipeline calls this after every edit to reload
/// PDF.js so the main view reflects in-memory edits (rotate, ink, text box, …)
/// without a save/reopen. `serde`'s default `Vec<u8>` serializer emits an
/// array-of-numbers, so a 13 MB document ballooned to ~50 MB of JSON text on
/// every edit — slow enough on large files that the reload never landed and
/// edits silently failed to appear (P4.HF28). Raw bytes are ~1× overhead.
/// Read-only — no mutation, no dirty change.
#[tauri::command]
pub async fn pdf_get_bytes(
    state: State<'_, AppState>,
    id: String,
) -> Result<tauri::ipc::Response, CommandError> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;

    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.get_bytes_request()?
    };

    let bytes = rx
        .await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))??;
    Ok(tauri::ipc::Response::new(bytes))
}

/// SPEC: P2-PAGE-007 — split the document into multiple PDFs under
/// `dest_dir`, named `{stem}-NNN.pdf`. Read-only on the source; returns one
/// write outcome per output file.
#[tauri::command]
pub async fn pdf_split_document(
    state: State<'_, AppState>,
    id: String,
    mode: SplitMode,
    dest_dir: String,
    stem: String,
) -> Result<SplitOutcome, CommandError> {
    if dest_dir.trim().is_empty() {
        return Err(CommandError::InvalidInput("no output directory".into()));
    }
    if stem.trim().is_empty() {
        return Err(CommandError::InvalidInput("no output file name".into()));
    }

    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;

    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.split_document_request(mode, PathBuf::from(dest_dir), stem)?
    };

    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P2-PAGE-008 (partial) — merge `paths` (≥ 2, in order) into a new PDF
/// at `dest`. Standalone: reads files from disk, not an open document (see
/// `docs/04_ARCHITECTURE.md` §"Stateless multi-file operations"). Runs the
/// blocking `PDFium` work on a `spawn_blocking` thread.
#[tauri::command]
pub async fn pdf_merge_documents(
    paths: Vec<String>,
    dest: String,
) -> Result<SaveOutcome, CommandError> {
    if paths.len() < 2 {
        return Err(CommandError::InvalidInput(
            "merge needs at least two files".into(),
        ));
    }
    if dest.trim().is_empty() {
        return Err(CommandError::InvalidInput("no output file".into()));
    }

    let sources: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    let dest = PathBuf::from(dest);

    tokio::task::spawn_blocking(move || merge_documents(&sources, &dest))
        .await
        .map_err(|e| CommandError::Internal(format!("merge task panicked: {e}")))?
}

/// SPEC: P2-PAGE-002 — reorder the pages of document `id` by `new_order`
/// (`new_order[new_pos] = old_index`, a permutation of `0..page_count`).
/// Undoable, mutating edit routed through the document actor.
#[tauri::command]
pub async fn pdf_reorder_pages(
    state: State<'_, AppState>,
    id: String,
    new_order: Vec<usize>,
) -> Result<HistoryState, CommandError> {
    if new_order.is_empty() {
        return Err(CommandError::InvalidInput("no page order specified".into()));
    }

    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;

    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.reorder_pages_request(new_order)?
    };

    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P2-PAGE-005 — insert `pages` (0-based) of the file at `source_path`
/// into the open document `id` at `index`. Undoable, mutating edit routed
/// through the document actor.
#[tauri::command]
pub async fn pdf_insert_from_pdf(
    state: State<'_, AppState>,
    id: String,
    source_path: String,
    pages: Vec<i32>,
    index: i32,
) -> Result<HistoryState, CommandError> {
    if source_path.trim().is_empty() {
        return Err(CommandError::InvalidInput("no source file".into()));
    }
    if pages.is_empty() {
        return Err(CommandError::InvalidInput("no pages specified".into()));
    }

    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;

    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.insert_from_pdf_request(PathBuf::from(source_path), pages, index)?
    };

    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// SPEC: P2-PAGE-005 (support) — read a file's page count without opening it as
/// a document (no actor). Used by the insert-from dialog to show the source's
/// length and validate the page range. Read-only standalone op (see `docs/04`
/// §"Stateless multi-file operations").
#[tauri::command]
pub async fn pdf_peek_page_count(path: String) -> Result<u32, CommandError> {
    if path.trim().is_empty() {
        return Err(CommandError::InvalidInput("no file path".into()));
    }
    tokio::task::spawn_blocking(move || {
        open_document_metadata(&PathBuf::from(path)).map(|m| m.page_count)
    })
    .await
    .map_err(|e| CommandError::Internal(format!("peek task panicked: {e}")))?
}
