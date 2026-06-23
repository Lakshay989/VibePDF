use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::error::CommandError;
use crate::pdf::actor::DocumentActorHandle;
use crate::pdf::cos::{AnnotationInfo, FreeTextData, MeasureCalibration, NoteData};
use crate::pdf::document::{open_document_metadata, SaveOutcome};
use crate::pdf::merge::merge_documents;
use crate::pdf::render::{ImageFormat, RenderedPage};
use crate::pdf::split::{SplitMode, SplitOutcome};
use crate::pdf::undo::HistoryState;
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
        handle.update_free_text_request(nm, text, font_family, font_size, color, bold, italic)?
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
        handle.add_free_text_request(page, rect, text, font_family, font_size, color, bold, italic)?
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

/// Serialize the live in-memory document to bytes (returned as a
/// `number[]` over IPC). The edit-preview pipeline reloads PDF.js from
/// these so the main view reflects in-memory edits (rotate, …) without a
/// save/reopen. Read-only — no mutation, no dirty change.
#[tauri::command]
pub async fn pdf_get_bytes(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<u8>, CommandError> {
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

    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
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
