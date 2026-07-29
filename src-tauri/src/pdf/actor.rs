//! Per-document actor.
//!
//! Every open PDF lives behind its own `std::thread`. The thread owns a
//! live `PdfDocument`, receives `Message`s through an `std::sync::mpsc`
//! mailbox, and answers each one via a `tokio::sync::oneshot` reply
//! channel embedded in the message variant.
//!
//! See `docs/04_ARCHITECTURE.md` § "The document actor" for the why.
//!
//! ## Threading model
//!
//! - **Mailbox:** `std::sync::mpsc::Sender<Message>` — synchronous,
//!   non-blocking on send.
//! - **Reply:** `tokio::sync::oneshot::Sender<T>` per request —
//!   crosses the sync→async boundary cheaply (`send` is sync; `await`
//!   on the receiver suspends the caller).
//! - **Worker:** dedicated OS thread named `doc-actor:<uuid>`. Holds the
//!   `PdfDocument<'static>` and a `tracing` span for the document's
//!   lifetime. Exits cleanly when every `Sender` is dropped or on
//!   explicit `Message::Close`.

use std::path::PathBuf;
use std::sync::mpsc;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::error::CommandError;
use crate::pdf::autosave;
use crate::pdf::crop::CropEdit;
use crate::pdf::delete_page::DeleteEdit;
use crate::pdf::extract::extract_pages;
use crate::pdf::insert_blank::InsertBlankEdit;
use crate::pdf::insert_from::InsertFromEdit;
use crate::pdf::document::{
    collect_metadata, open_pdf, pdfium_lock, save_document, DocumentMetadata, SaveOutcome,
};
use crate::pdf::render::{self, ImageFormat, RenderedPage};
use crate::pdf::annotation::{
    AddImageEdit, AddLinkEdit, AddNoteEdit, ClearMarkupEdit, DeleteAnnotationEdit, FlattenEdit,
    FreeTextEdit, ImageStampEdit, ImportXfdfEdit, InkEdit, LineEdit, MeasureEdit, PolygonEdit,
    RemoveTextBoxEdit, ReplyEdit, StampEdit, ShapeEdit, TextBoxEdit, TextMarkupEdit,
    UpdateFreeTextEdit, UpdateNoteEdit, UpdateTextBoxEdit,
};
use crate::pdf::background::{BackgroundEdit, BackgroundKind};
use crate::pdf::header_footer::HeaderFooterEdit;
use crate::pdf::watermark::{RemoveWatermarksEdit, WatermarkEdit, WatermarkKind};
use crate::pdf::cos::{
    read_annotations_doc, read_free_text_doc, read_measure_calibration_doc, read_text_boxes_doc,
    read_text_notes_doc, AnnotationInfo, FreeTextData, MeasureCalibration, NoteData, TextBoxInfo,
};
use crate::pdf::doc_cache::CachedDoc;
use crate::pdf::font_resolver::{build_font_report, FontReport};
use crate::pdf::image_edit::{DeleteImageEdit, ReplaceImageEdit, TransformImageEdit};
use crate::pdf::image_extract::{extract_images, ImageInfo};
use crate::pdf::reflow::{DeleteTextRunEdit, ReplaceTextRunEdit};
use crate::pdf::text_extract::{collect_document_fonts, extract_text_runs, TextRun};
use crate::pdf::xfdf::annotations_to_xfdf;
use crate::pdf::reorder::ReorderEdit;
use crate::pdf::resize::ResizeEdit;
use crate::pdf::rotate::RotateEdit;
use crate::pdf::split::{split_document, SplitMode, SplitOutcome};
use crate::pdf::undo::{Edit, HistoryState, UndoStack};

/// Event name on the wire. The frontend listens for this via
/// `tauri::event::listen("document-changed", ...)`.
pub const DOCUMENT_CHANGED_EVENT: &str = "document-changed";

/// Payload of the `document-changed` event. Kebab-case `kind` tag
/// matches the rest of the wire model (see `docs/06_CONVENTIONS.md`).
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum DocumentChange {
    Opened { id: String, page_count: u32 },
    Closed { id: String },
}

/// Messages the worker thread accepts. Each variant carries its own
/// reply channel so the worker can answer one message at a time
/// without head-of-line blocking on the mailbox.
///
/// `RenderThumbnail` and `RenderPage` both return a `RenderedPage`
/// from `pdf::render`; the difference is the size selector
/// (pixel-width vs DPI) and whether the bytes come back PNG-encoded.
pub enum Message {
    GetPageCount {
        reply: oneshot::Sender<u32>,
    },
    GetMetadata {
        reply: oneshot::Sender<DocumentMetadata>,
    },
    RenderThumbnail {
        page: u32,
        max_width: u32,
        reply: oneshot::Sender<Result<RenderedPage, CommandError>>,
    },
    RenderPage {
        page: u32,
        dpi: f32,
        format: ImageFormat,
        reply: oneshot::Sender<Result<RenderedPage, CommandError>>,
    },
    /// SPEC: P4-EDIT-001 (P4.A1) — extract every text run on a page (read-only,
    /// on the live `PdfDocument` under the `PDFium` lock). Feeds click-to-edit.
    ReadTextRuns {
        page: usize,
        reply: oneshot::Sender<Result<Vec<TextRun>, CommandError>>,
    },
    /// SPEC: P4-EDIT-002 (P4.A2) — resolve the document's fonts against the
    /// system to decide which edits would be lossy (read-only; same lock path).
    ReadFontReport {
        reply: oneshot::Sender<Result<FontReport, CommandError>>,
    },
    /// SPEC: P4-EDIT-001 (P4.B1) — replace run `run_index` on `page` with
    /// `new_text`, preserving its font/size/colour/matrix (in-place `set_text`).
    /// Undoable; marks dirty; replies with history availability.
    ReplaceTextRun {
        page: usize,
        run_index: usize,
        new_text: String,
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P4-EDIT-004 (P4.B3) — remove run `run_index` on `page` from the page
    /// content stream (lopdf splice + verify). Undoable; marks dirty.
    DeleteTextRun {
        page: usize,
        run_index: usize,
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P4-EDIT-006 (P4.C2) — locate the images on `page` (read-only).
    ReadImages {
        page: usize,
        reply: oneshot::Sender<Result<Vec<ImageInfo>, CommandError>>,
    },
    /// SPEC: P4-EDIT-006 (P4.C2) — override image `index`'s placement matrix on
    /// `page` (move/resize/rotate). Undoable; marks dirty.
    TransformImage {
        page: usize,
        index: usize,
        matrix: [f32; 6],
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P4-EDIT-006 (P4.C2) — delete image `index` on `page` (lopdf splice +
    /// verify). Undoable; marks dirty.
    DeleteImage {
        page: usize,
        index: usize,
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P4-EDIT-006 (P4.C2b) — replace image `index`'s pixels on `page` with a
    /// new PNG/JPEG, preserving placement. Undoable; marks dirty.
    ReplaceImage {
        page: usize,
        index: usize,
        image: Vec<u8>,
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P2-SAVE-001 — explicit save. `path = None` writes back to
    /// the document's own path; `Some(p)` is a save-as to `p`.
    Save {
        path: Option<PathBuf>,
        reply: oneshot::Sender<Result<SaveOutcome, CommandError>>,
    },
    /// SPEC: P2-PAGE-003 / session history — undo the most recent edit.
    /// No-op (returns the unchanged state) when nothing is undoable.
    Undo {
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// Redo the most recently undone edit. No-op when nothing is redoable.
    Redo {
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// Query current undo/redo availability (for UI button state).
    GetHistoryState {
        reply: oneshot::Sender<HistoryState>,
    },
    /// Serialize the live document to bytes — the edit-preview pipeline
    /// reloads PDF.js from these so the view reflects in-memory edits
    /// without a save/reopen.
    GetBytes {
        reply: oneshot::Sender<Result<Vec<u8>, CommandError>>,
    },
    /// SPEC: P2-PAGE-001 — rotate `pages` by `quarter_turns` × 90°.
    /// Applies the edit, records its inverse on the undo stack, and marks
    /// the document dirty; replies with the new history availability.
    RotatePages {
        pages: Vec<i32>,
        quarter_turns: i32,
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P2-PAGE-003 — delete `pages` (0-based indices). Records a
    /// content-preserving inverse on the undo stack and marks dirty.
    DeletePages {
        pages: Vec<i32>,
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P2-PAGE-004 — insert a blank page at `index` (0-based; `index
    /// == page_count` appends). `size` (w, h in points) overrides the
    /// inherited adjacent-page dimensions. Undoable, marks dirty.
    InsertBlankPage {
        index: i32,
        size: Option<(f32, f32)>,
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P2-PAGE-009 — crop `page` to `rect` (left, bottom, right, top
    /// in points), or reset to the `MediaBox` when `None`. Undoable, dirty.
    CropPage {
        page: i32,
        rect: Option<(f32, f32, f32, f32)>,
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P2-PAGE-002 — reorder pages; `order[new_pos] = old_index` (a
    /// permutation of `0..page_count`). Undoable, marks dirty.
    ReorderPages {
        order: Vec<usize>,
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P2-PAGE-010 — resize `pages` (0-based) to `width` × `height`
    /// points, scaling content to fit (`preserve_aspect` centres it under a
    /// uniform scale). Undoable (snapshot inverse), marks dirty.
    ResizePages {
        pages: Vec<i32>,
        width: f32,
        height: f32,
        preserve_aspect: bool,
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P3-ANN-001 — add a text-markup annotation (`subtype` =
    /// highlight/underline/strikethrough/squiggly) over `quads` on `page`.
    /// Undoable (snapshot inverse), marks dirty.
    AddTextMarkup {
        page: i32,
        subtype: String,
        quads: Vec<[f32; 8]>,
        color: String,
        opacity: f32,
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P3-ANN-001 — remove all text-markup annotations. Undoable, dirty.
    ClearTextMarkup {
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P3-ANN-002 — add a sticky note (`/Text`) at `(x, y)` on `page`.
    AddNote {
        note_id: String,
        page: i32,
        x: f32,
        y: f32,
        content: String,
        author: String,
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P3-ANN-009 — reply to the annotation `parent_id` (a `/Text` linked
    /// via `/IRT`). Undoable; marks dirty.
    AddReply {
        parent_id: String,
        author: String,
        content: String,
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P3-ANN-002 — update a note's body by `/NM`.
    UpdateNote {
        note_id: String,
        content: String,
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P3-ANN-002 — delete the annotation with `/NM == note_id`.
    DeleteAnnotation {
        note_id: String,
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P3-ANN-002 (re-openable) — read every sticky note out of the live
    /// document. Read-only (no edit, no history); the frontend projects the
    /// result into its note overlay on open and after undo/redo.
    ReadNotes {
        reply: oneshot::Sender<Result<Vec<NoteData>, CommandError>>,
    },
    /// SPEC: P3-ANN-008 — read every supported annotation for the sidebar list.
    /// Read-only; the panel pulls on open and after each edit epoch.
    ReadAnnotations {
        reply: oneshot::Sender<Result<Vec<AnnotationInfo>, CommandError>>,
    },
    /// SPEC: P3-ANN-010 — export every annotation to an XFDF file at `dest`.
    /// Read-only; replies with the count written.
    ExportAnnotations {
        dest: PathBuf,
        reply: oneshot::Sender<Result<usize, CommandError>>,
    },
    /// SPEC: P3-ANN-010 — import the annotations described by an XFDF document as
    /// one undoable edit; marks dirty.
    ImportXfdf {
        xfdf: String,
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P3-ANN-011 — flatten every `/AP`-bearing annotation into the page
    /// content streams. Undoable in-session; marks dirty.
    FlattenAnnotations {
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P3-ANN-013 — read one free-text annotation's text + style by `/NM`,
    /// so the in-place editor opens pre-filled. Read-only.
    ReadFreeText {
        nm: String,
        reply: oneshot::Sender<Result<Option<FreeTextData>, CommandError>>,
    },
    /// SPEC: P3-ANN-013 — update a free-text annotation (by `/NM`) in place.
    /// Undoable; marks dirty.
    UpdateFreeText {
        nm: String,
        text: String,
        font_family: String,
        font_size: f32,
        color: String,
        bold: bool,
        italic: bool,
        underline: bool,
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P3-ANN-003 — add a free-text box at `rect` on `page` with a
    /// generated `/AP`. Undoable; marks dirty; replies with history availability.
    AddFreeText {
        page: i32,
        rect: [f32; 4],
        text: String,
        font_family: String,
        font_size: f32,
        color: String,
        bold: bool,
        italic: bool,
        underline: bool,
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P4-EDIT-003 (P4.B2) — add a text box as **page content** (not an
    /// annotation) at `rect` on `page`. Undoable; marks dirty.
    AddTextBox {
        page: i32,
        rect: [f32; 4],
        text: String,
        font_family: String,
        font_size: f32,
        color: String,
        bold: bool,
        italic: bool,
        underline: bool,
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P4-EDIT-003b — re-edit the Add-Text box `id` on `page` (new text +
    /// style, same rect). Undoable; marks dirty.
    UpdateTextBox {
        page: i32,
        id: String,
        text: String,
        font_family: String,
        font_size: f32,
        color: String,
        bold: bool,
        italic: bool,
        underline: bool,
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P4-EDIT-003b — read every re-editable Add-Text box on `page`
    /// (read-only). Powers the double-click re-edit hit-testing.
    ReadTextBoxes {
        page: usize,
        reply: oneshot::Sender<Result<Vec<TextBoxInfo>, CommandError>>,
    },
    /// SPEC: P4-EDIT-003b / P4-EDIT-004 — delete the Add-Text box `id` on `page`.
    /// Undoable; marks dirty.
    RemoveTextBox {
        page: i32,
        id: String,
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P4-EDIT-005 (P4.C1) — add an image as **page content** (an Image
    /// `XObject`), aspect-fit into `rect` on `page`. Undoable; marks dirty.
    AddImage {
        page: i32,
        rect: [f32; 4],
        image: Vec<u8>,
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P4-EDIT-007 — add a `/Link` annotation over `rect`. `kind` is
    /// `url` | `email` | `page` | `named`; `value` is the matching target.
    /// SPEC: P4-EDIT-007b — `style` is `invisible` | `box` | `underline` in
    /// `color` (`#rrggbb`). Undoable; marks dirty.
    AddLink {
        page: i32,
        rect: [f32; 4],
        kind: String,
        value: String,
        style: String,
        color: String,
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P4-EDIT-009 — stamp a text/image watermark on `pages` (0-based) at
    /// `opacity` + `rotation`, on top or `behind` content. Undoable; marks dirty.
    AddWatermark {
        pages: Vec<i32>,
        kind: WatermarkKind,
        opacity: f32,
        rotation: f32,
        behind: bool,
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P4-EDIT-009 — remove every watermark this app added, from all pages.
    /// Undoable; marks dirty.
    RemoveWatermarks {
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P4-EDIT-008 — fill `pages` (0-based) behind their content with a
    /// colour or image at `opacity`. Undoable; marks dirty.
    AddBackground {
        pages: Vec<i32>,
        kind: BackgroundKind,
        opacity: f32,
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P4-EDIT-010 — draw left/center/right header or footer text (with
    /// `{n}`/`{total}`/`{date}` placeholders) on `pages` (0-based). Undoable.
    AddHeaderFooter {
        pages: Vec<i32>,
        position: String,
        left: String,
        center: String,
        right: String,
        font_family: String,
        size: f32,
        color: String,
        margin: f32,
        date: String,
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P3-ANN-004 — add a shape (`/Square` or `/Circle`) at `rect` with a
    /// generated `/AP`. Undoable; marks dirty.
    AddShape {
        page: i32,
        kind: String,
        rect: [f32; 4],
        stroke: String,
        fill: Option<String>,
        opacity: f32,
        stroke_width: f32,
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P3-ANN-004 — add a line (or arrow) annotation with a generated `/AP`.
    /// Undoable; marks dirty.
    AddLine {
        page: i32,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        arrow: bool,
        stroke: String,
        opacity: f32,
        stroke_width: f32,
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P3-ANN-004 — add a polygon / polyline annotation with a generated
    /// `/AP`. Undoable; marks dirty.
    AddPolygon {
        page: i32,
        closed: bool,
        points: Vec<[f32; 2]>,
        stroke: String,
        fill: Option<String>,
        opacity: f32,
        stroke_width: f32,
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P3-ANN-005 — add a freehand `/Ink` annotation with a generated
    /// `/AP`. Undoable; marks dirty.
    AddInk {
        page: i32,
        points: Vec<[f32; 3]>,
        color: String,
        opacity: f32,
        base_width: f32,
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P3-ANN-006 — add a `/Stamp` annotation with a generated `/AP`.
    /// Undoable; marks dirty.
    AddStamp {
        page: i32,
        rect: [f32; 4],
        text: String,
        name: String,
        color: String,
        opacity: f32,
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P3-ANN-006 (P3.C3b) — add an image `/Stamp` (PNG bytes embedded as an
    /// Image `XObject`). Undoable; marks dirty.
    AddImageStamp {
        page: i32,
        x: f32,
        y: f32,
        height: f32,
        image: Vec<u8>,
        text: Option<String>,
        opacity: f32,
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P3-ANN-007 — add a measurement annotation with a generated `/AP`.
    /// Undoable; marks dirty.
    AddMeasure {
        page: i32,
        kind: String,
        points: Vec<[f32; 2]>,
        color: String,
        label: String,
        opacity: f32,
        stroke_width: f32,
        units_per_point: f32,
        unit: String,
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P3-ANN-007 (P3.C4b) — read a measurement's persisted calibration
    /// from its `/Measure` dict, to re-seed the tool on reopen. Read-only.
    ReadMeasureCalibration {
        reply: oneshot::Sender<Result<Option<MeasureCalibration>, CommandError>>,
    },
    /// SPEC: P2-PAGE-005 — insert `pages` (0-based) of the file at
    /// `source_path` into the open document at `index`. Undoable, marks dirty.
    InsertFromPdf {
        source_path: PathBuf,
        pages: Vec<i32>,
        index: i32,
        reply: oneshot::Sender<Result<HistoryState, CommandError>>,
    },
    /// SPEC: P2-PAGE-006 — extract `pages` (0-based) into a new PDF at
    /// `dest`. Read-only on the source: no mutation, no undo, no dirty.
    ExtractPages {
        pages: Vec<i32>,
        dest: PathBuf,
        reply: oneshot::Sender<Result<SaveOutcome, CommandError>>,
    },
    /// SPEC: P2-PAGE-007 — split into multiple files under `dest_dir`,
    /// named `{stem}-NNN.pdf`. Read-only on the source: no undo, no dirty.
    SplitDocument {
        mode: SplitMode,
        dest_dir: PathBuf,
        stem: String,
        reply: oneshot::Sender<Result<SplitOutcome, CommandError>>,
    },
    /// SPEC: P2.A2 — fire-and-forget poke from the autosave tick. Writes a
    /// recovery copy iff the document is dirty; no reply (best-effort).
    Autosave,
    Close,
}

/// The frontend-facing handle. Cloning is intentionally disallowed —
/// the handle owns the mailbox sender, and dropping it signals the
/// worker to exit. `Debug` is derived so tests can use `expect_err`,
/// which formats the unexpected `Ok` variant.
#[derive(Debug)]
pub struct DocumentActorHandle {
    id: Uuid,
    path: PathBuf,
    tx: mpsc::Sender<Message>,
    metadata: DocumentMetadata,
}

impl DocumentActorHandle {
    /// Spawn a dedicated worker thread that opens `path` and owns the
    /// resulting `PdfDocument` for its lifetime.
    ///
    /// `app` is optional so integration tests can spawn actors without
    /// a real Tauri app. In production it is always `Some(handle)`.
    ///
    /// SPEC: P1-VIEW-001 — open via actor.
    /// SPEC: P1-VIEW-002 — open errors propagate as typed `CommandError`,
    /// never as a panic that takes down the process.
    pub fn spawn(
        app: Option<AppHandle>,
        id: Uuid,
        path: PathBuf,
        password: Option<String>,
    ) -> Result<Self, CommandError> {
        let (tx, rx) = mpsc::channel::<Message>();
        let (ready_tx, ready_rx) =
            std::sync::mpsc::channel::<Result<DocumentMetadata, CommandError>>();

        let thread_path = path.clone();
        std::thread::Builder::new()
            .name(format!("doc-actor:{id}"))
            .spawn(move || run_worker(app, id, thread_path, password, rx, ready_tx))
            .map_err(|e| CommandError::Internal(format!("doc-actor thread spawn failed: {e}")))?;

        // The worker reports open success or failure back through this
        // one-shot synchronous channel; from here on, every other reply
        // goes through tokio::sync::oneshot.
        let metadata = ready_rx
            .recv()
            .map_err(|_| CommandError::Internal("doc-actor died before ready".into()))??;

        Ok(Self {
            id,
            path,
            tx,
            metadata,
        })
    }

    #[must_use]
    pub fn id(&self) -> Uuid {
        self.id
    }

    #[must_use]
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Cached metadata captured at open. Avoids a mailbox round-trip
    /// for `pdf_open`'s response payload.
    #[must_use]
    pub fn metadata(&self) -> &DocumentMetadata {
        &self.metadata
    }

    pub async fn page_count(&self) -> Result<u32, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::GetPageCount { reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))
    }

    pub async fn metadata_live(&self) -> Result<DocumentMetadata, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::GetMetadata { reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))
    }

    pub async fn render_thumbnail(
        &self,
        page: u32,
        max_width: u32,
    ) -> Result<RenderedPage, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::RenderThumbnail {
                page,
                max_width,
                reply,
            })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    /// SPEC: P1-VIEW-008 + NFR-PERF-003 — thumbnail and viewer
    /// pipelines both consume this. PNG path is encoded inside the
    /// actor thread (read-side; no contention with writes). RGBA8
    /// path skips encoding for canvas consumers.
    ///
    /// Convenience version that holds `&self` across the await; fine
    /// for tests, **not** fine for IPC handlers that pulled the
    /// handle out of `Mutex<HashMap<…>>`. Those should call
    /// `render_page_request` instead.
    pub async fn render_page(
        &self,
        page: u32,
        dpi: f32,
        format: ImageFormat,
    ) -> Result<RenderedPage, CommandError> {
        let rx = self.render_page_request(page, dpi, format)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    /// Send-only sibling of `render_page`. Returns the reply receiver
    /// without borrowing `&self` across the await — lets the caller
    /// release the actor-map lock before awaiting.
    pub fn render_page_request(
        &self,
        page: u32,
        dpi: f32,
        format: ImageFormat,
    ) -> Result<oneshot::Receiver<Result<RenderedPage, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::RenderPage {
                page,
                dpi,
                format,
                reply,
            })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P2-SAVE-001 — explicit save. `path = None` saves to the
    /// document's own path (a no-op when there are no unsaved changes);
    /// `Some(p)` is a save-as.
    ///
    /// Convenience version that holds `&self` across the await; use it
    /// from tests. IPC handlers should call `save_request` so they can
    /// drop the actor-map lock before awaiting (see `render_page` vs
    /// `render_page_request`).
    pub async fn save(&self, path: Option<PathBuf>) -> Result<SaveOutcome, CommandError> {
        let rx = self.save_request(path)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    /// Send-only sibling of `save`. Returns the reply receiver without
    /// borrowing `&self` across the await.
    pub fn save_request(
        &self,
        path: Option<PathBuf>,
    ) -> Result<oneshot::Receiver<Result<SaveOutcome, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::Save { path, reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P2-PAGE-003 / session history — undo the most recent edit.
    /// Await-holding convenience for tests; IPC handlers use
    /// `undo_request` so they can drop the actor-map lock before awaiting.
    pub async fn undo(&self) -> Result<HistoryState, CommandError> {
        let rx = self.undo_request()?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn undo_request(
        &self,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::Undo { reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// Redo the most recently undone edit. See `undo` for the await note.
    pub async fn redo(&self) -> Result<HistoryState, CommandError> {
        let rx = self.redo_request()?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn redo_request(
        &self,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::Redo { reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// Current undo/redo availability. Await-holding convenience for
    /// tests; IPC uses `history_state_request`.
    pub async fn history_state(&self) -> Result<HistoryState, CommandError> {
        let rx = self.history_state_request()?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))
    }

    pub fn history_state_request(
        &self,
    ) -> Result<oneshot::Receiver<HistoryState>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::GetHistoryState { reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// Serialize the live document to bytes (edit-preview pipeline).
    /// Await-holding convenience for tests; IPC uses `get_bytes_request`.
    pub async fn get_bytes(&self) -> Result<Vec<u8>, CommandError> {
        let rx = self.get_bytes_request()?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn get_bytes_request(
        &self,
    ) -> Result<oneshot::Receiver<Result<Vec<u8>, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::GetBytes { reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P2.A2 — fire-and-forget autosave poke from the tick thread.
    /// The actor writes a recovery copy iff dirty; a closed mailbox (the
    /// worker already exited) is silently ignored.
    pub fn poke_autosave(&self) {
        let _ = self.tx.send(Message::Autosave);
    }

    /// SPEC: P2-PAGE-001 — rotate `pages` by `quarter_turns` × 90°.
    /// Await-holding convenience for tests; IPC uses `rotate_pages_request`.
    pub async fn rotate_pages(
        &self,
        pages: Vec<i32>,
        quarter_turns: i32,
    ) -> Result<HistoryState, CommandError> {
        let rx = self.rotate_pages_request(pages, quarter_turns)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn rotate_pages_request(
        &self,
        pages: Vec<i32>,
        quarter_turns: i32,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::RotatePages {
                pages,
                quarter_turns,
                reply,
            })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P2-PAGE-010 — resize `pages` to `width` × `height` points.
    /// Await-holding convenience for tests; IPC uses `resize_pages_request`.
    pub async fn resize_pages(
        &self,
        pages: Vec<i32>,
        width: f32,
        height: f32,
        preserve_aspect: bool,
    ) -> Result<HistoryState, CommandError> {
        let rx = self.resize_pages_request(pages, width, height, preserve_aspect)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn resize_pages_request(
        &self,
        pages: Vec<i32>,
        width: f32,
        height: f32,
        preserve_aspect: bool,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::ResizePages {
                pages,
                width,
                height,
                preserve_aspect,
                reply,
            })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P3-ANN-001 — add a text-markup annotation. Await-holding
    /// convenience for tests; IPC uses `add_text_markup_request`.
    pub async fn add_text_markup(
        &self,
        page: i32,
        subtype: String,
        quads: Vec<[f32; 8]>,
        color: String,
        opacity: f32,
    ) -> Result<HistoryState, CommandError> {
        let rx = self.add_text_markup_request(page, subtype, quads, color, opacity)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn add_text_markup_request(
        &self,
        page: i32,
        subtype: String,
        quads: Vec<[f32; 8]>,
        color: String,
        opacity: f32,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::AddTextMarkup {
                page,
                subtype,
                quads,
                color,
                opacity,
                reply,
            })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P3-ANN-001 — remove all text-markup annotations. Await-holding
    /// convenience for tests; IPC uses `clear_text_markup_request`.
    pub async fn clear_text_markup(&self) -> Result<HistoryState, CommandError> {
        let rx = self.clear_text_markup_request()?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn clear_text_markup_request(
        &self,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::ClearTextMarkup { reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P3-ANN-002 — add a sticky note. Await-holding convenience for tests.
    pub async fn add_note(
        &self,
        note_id: String,
        page: i32,
        x: f32,
        y: f32,
        content: String,
        author: String,
    ) -> Result<HistoryState, CommandError> {
        let rx = self.add_note_request(note_id, page, x, y, content, author)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn add_note_request(
        &self,
        note_id: String,
        page: i32,
        x: f32,
        y: f32,
        content: String,
        author: String,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::AddNote { note_id, page, x, y, content, author, reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P3-ANN-009 — reply to an annotation. Await-holding for tests.
    pub async fn add_reply(
        &self,
        parent_id: String,
        author: String,
        content: String,
    ) -> Result<HistoryState, CommandError> {
        let rx = self.add_reply_request(parent_id, author, content)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn add_reply_request(
        &self,
        parent_id: String,
        author: String,
        content: String,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::AddReply { parent_id, author, content, reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P3-ANN-002 — update a note's body. Await-holding for tests.
    pub async fn update_note(&self, note_id: String, content: String) -> Result<HistoryState, CommandError> {
        let rx = self.update_note_request(note_id, content)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn update_note_request(
        &self,
        note_id: String,
        content: String,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::UpdateNote { note_id, content, reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P3-ANN-002 — delete an annotation by `/NM`. Await-holding for tests.
    pub async fn delete_annotation(&self, note_id: String) -> Result<HistoryState, CommandError> {
        let rx = self.delete_annotation_request(note_id)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn delete_annotation_request(
        &self,
        note_id: String,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::DeleteAnnotation { note_id, reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P3-ANN-002 (re-openable) — read every sticky note. Await-holding
    /// convenience for tests; IPC uses `read_notes_request`.
    pub async fn read_notes(&self) -> Result<Vec<NoteData>, CommandError> {
        let rx = self.read_notes_request()?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn read_notes_request(
        &self,
    ) -> Result<oneshot::Receiver<Result<Vec<NoteData>, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::ReadNotes { reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P3-ANN-008 — read every supported annotation. Await-holding for tests.
    pub async fn read_annotations(&self) -> Result<Vec<AnnotationInfo>, CommandError> {
        let rx = self.read_annotations_request()?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn read_annotations_request(
        &self,
    ) -> Result<oneshot::Receiver<Result<Vec<AnnotationInfo>, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::ReadAnnotations { reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P3-ANN-010 — export every annotation to an XFDF file. Await-holding
    /// convenience for tests; IPC uses `export_annotations_request`.
    pub async fn export_annotations(&self, dest: PathBuf) -> Result<usize, CommandError> {
        let rx = self.export_annotations_request(dest)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn export_annotations_request(
        &self,
        dest: PathBuf,
    ) -> Result<oneshot::Receiver<Result<usize, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::ExportAnnotations { dest, reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P3-ANN-010 — import annotations from an XFDF document. Await-holding
    /// convenience for tests; IPC uses `import_xfdf_request`.
    pub async fn import_xfdf(&self, xfdf: String) -> Result<HistoryState, CommandError> {
        let rx = self.import_xfdf_request(xfdf)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn import_xfdf_request(
        &self,
        xfdf: String,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::ImportXfdf { xfdf, reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P3-ANN-011 — flatten annotations into the page content. Await-holding
    /// convenience for tests; IPC uses `flatten_annotations_request`.
    pub async fn flatten_annotations(&self) -> Result<HistoryState, CommandError> {
        let rx = self.flatten_annotations_request()?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn flatten_annotations_request(
        &self,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::FlattenAnnotations { reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P3-ANN-013 — read one free-text annotation's editable state.
    pub async fn read_free_text(&self, nm: String) -> Result<Option<FreeTextData>, CommandError> {
        let rx = self.read_free_text_request(nm)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn read_free_text_request(
        &self,
        nm: String,
    ) -> Result<oneshot::Receiver<Result<Option<FreeTextData>, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::ReadFreeText { nm, reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P3-ANN-013 — update a free-text annotation in place. Await-holding.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_free_text(
        &self,
        nm: String,
        text: String,
        font_family: String,
        font_size: f32,
        color: String,
        bold: bool,
        italic: bool,
        underline: bool,
    ) -> Result<HistoryState, CommandError> {
        let rx = self.update_free_text_request(
            nm, text, font_family, font_size, color, bold, italic, underline,
        )?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_free_text_request(
        &self,
        nm: String,
        text: String,
        font_family: String,
        font_size: f32,
        color: String,
        bold: bool,
        italic: bool,
        underline: bool,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::UpdateFreeText {
                nm,
                text,
                font_family,
                font_size,
                color,
                bold,
                italic,
                underline,
                reply,
            })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P3-ANN-003 — add a free-text box. Await-holding for tests.
    #[allow(clippy::too_many_arguments)]
    pub async fn add_free_text(
        &self,
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
        let rx = self.add_free_text_request(
            page, rect, text, font_family, font_size, color, bold, italic, underline,
        )?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_free_text_request(
        &self,
        page: i32,
        rect: [f32; 4],
        text: String,
        font_family: String,
        font_size: f32,
        color: String,
        bold: bool,
        italic: bool,
        underline: bool,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::AddFreeText {
                page,
                rect,
                text,
                font_family,
                font_size,
                color,
                bold,
                italic,
                underline,
                reply,
            })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P4-EDIT-003 (P4.B2) — add a content-stream text box. Await-holding for tests.
    #[allow(clippy::too_many_arguments)]
    pub async fn add_text_box(
        &self,
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
        let rx = self.add_text_box_request(
            page, rect, text, font_family, font_size, color, bold, italic, underline,
        )?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_text_box_request(
        &self,
        page: i32,
        rect: [f32; 4],
        text: String,
        font_family: String,
        font_size: f32,
        color: String,
        bold: bool,
        italic: bool,
        underline: bool,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::AddTextBox {
                page,
                rect,
                text,
                font_family,
                font_size,
                color,
                bold,
                italic,
                underline,
                reply,
            })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P4-EDIT-003b — re-edit an Add-Text box in place. Await-holding for tests.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_text_box(
        &self,
        page: i32,
        id: String,
        text: String,
        font_family: String,
        font_size: f32,
        color: String,
        bold: bool,
        italic: bool,
        underline: bool,
    ) -> Result<HistoryState, CommandError> {
        let rx = self.update_text_box_request(
            page, id, text, font_family, font_size, color, bold, italic, underline,
        )?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_text_box_request(
        &self,
        page: i32,
        id: String,
        text: String,
        font_family: String,
        font_size: f32,
        color: String,
        bold: bool,
        italic: bool,
        underline: bool,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::UpdateTextBox {
                page,
                id,
                text,
                font_family,
                font_size,
                color,
                bold,
                italic,
                underline,
                reply,
            })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P4-EDIT-003b — read this page's re-editable Add-Text boxes. Read-only.
    pub async fn read_text_boxes(&self, page: usize) -> Result<Vec<TextBoxInfo>, CommandError> {
        let rx = self.read_text_boxes_request(page)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn read_text_boxes_request(
        &self,
        page: usize,
    ) -> Result<oneshot::Receiver<Result<Vec<TextBoxInfo>, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::ReadTextBoxes { page, reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P4-EDIT-003b — delete an Add-Text box by its `/Id`. Await-holding for tests.
    pub async fn delete_text_box(&self, page: i32, id: String) -> Result<HistoryState, CommandError> {
        let rx = self.delete_text_box_request(page, id)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn delete_text_box_request(
        &self,
        page: i32,
        id: String,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::RemoveTextBox { page, id, reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P4-EDIT-005 (P4.C1) — add an image as page content. Await-holding for tests.
    pub async fn add_image(
        &self,
        page: i32,
        rect: [f32; 4],
        image: Vec<u8>,
    ) -> Result<HistoryState, CommandError> {
        let rx = self.add_image_request(page, rect, image)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn add_image_request(
        &self,
        page: i32,
        rect: [f32; 4],
        image: Vec<u8>,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::AddImage {
                page,
                rect,
                image,
                reply,
            })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P4-EDIT-007 (P4.C3) — add a `/Link` annotation. Await-holding for tests.
    #[allow(clippy::too_many_arguments)]
    pub async fn add_link(
        &self,
        page: i32,
        rect: [f32; 4],
        kind: String,
        value: String,
        style: String,
        color: String,
    ) -> Result<HistoryState, CommandError> {
        let rx = self.add_link_request(page, rect, kind, value, style, color)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_link_request(
        &self,
        page: i32,
        rect: [f32; 4],
        kind: String,
        value: String,
        style: String,
        color: String,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::AddLink {
                page,
                rect,
                kind,
                value,
                style,
                color,
                reply,
            })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P4-EDIT-009 (P4.D2) — stamp a watermark. Await-holding for tests.
    pub async fn add_watermark(
        &self,
        pages: Vec<i32>,
        kind: WatermarkKind,
        opacity: f32,
        rotation: f32,
        behind: bool,
    ) -> Result<HistoryState, CommandError> {
        let rx = self.add_watermark_request(pages, kind, opacity, rotation, behind)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn add_watermark_request(
        &self,
        pages: Vec<i32>,
        kind: WatermarkKind,
        opacity: f32,
        rotation: f32,
        behind: bool,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::AddWatermark {
                pages,
                kind,
                opacity,
                rotation,
                behind,
                reply,
            })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P4-EDIT-009 — remove all watermarks. Await-holding for tests.
    pub async fn remove_watermarks(&self) -> Result<HistoryState, CommandError> {
        let rx = self.remove_watermarks_request()?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn remove_watermarks_request(
        &self,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::RemoveWatermarks { reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P4-EDIT-008 (P4.D1) — fill pages behind content. Await-holding for tests.
    pub async fn add_background(
        &self,
        pages: Vec<i32>,
        kind: BackgroundKind,
        opacity: f32,
    ) -> Result<HistoryState, CommandError> {
        let rx = self.add_background_request(pages, kind, opacity)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn add_background_request(
        &self,
        pages: Vec<i32>,
        kind: BackgroundKind,
        opacity: f32,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::AddBackground {
                pages,
                kind,
                opacity,
                reply,
            })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P4-EDIT-010 (P4.D3) — draw a header/footer. Await-holding for tests.
    #[allow(clippy::too_many_arguments)]
    pub async fn add_header_footer(
        &self,
        pages: Vec<i32>,
        position: String,
        left: String,
        center: String,
        right: String,
        font_family: String,
        size: f32,
        color: String,
        margin: f32,
        date: String,
    ) -> Result<HistoryState, CommandError> {
        let rx = self.add_header_footer_request(
            pages, position, left, center, right, font_family, size, color, margin, date,
        )?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_header_footer_request(
        &self,
        pages: Vec<i32>,
        position: String,
        left: String,
        center: String,
        right: String,
        font_family: String,
        size: f32,
        color: String,
        margin: f32,
        date: String,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::AddHeaderFooter {
                pages,
                position,
                left,
                center,
                right,
                font_family,
                size,
                color,
                margin,
                date,
                reply,
            })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P3-ANN-004 — add a shape annotation. Await-holding for tests.
    #[allow(clippy::too_many_arguments)]
    pub async fn add_shape(
        &self,
        page: i32,
        kind: String,
        rect: [f32; 4],
        stroke: String,
        fill: Option<String>,
        opacity: f32,
        stroke_width: f32,
    ) -> Result<HistoryState, CommandError> {
        let rx = self.add_shape_request(page, kind, rect, stroke, fill, opacity, stroke_width)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_shape_request(
        &self,
        page: i32,
        kind: String,
        rect: [f32; 4],
        stroke: String,
        fill: Option<String>,
        opacity: f32,
        stroke_width: f32,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::AddShape {
                page,
                kind,
                rect,
                stroke,
                fill,
                opacity,
                stroke_width,
                reply,
            })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P3-ANN-004 — add a line (or arrow). Await-holding for tests.
    #[allow(clippy::too_many_arguments)]
    pub async fn add_line(
        &self,
        page: i32,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        arrow: bool,
        stroke: String,
        opacity: f32,
        stroke_width: f32,
    ) -> Result<HistoryState, CommandError> {
        let rx =
            self.add_line_request(page, x1, y1, x2, y2, arrow, stroke, opacity, stroke_width)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_line_request(
        &self,
        page: i32,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        arrow: bool,
        stroke: String,
        opacity: f32,
        stroke_width: f32,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::AddLine {
                page,
                x1,
                y1,
                x2,
                y2,
                arrow,
                stroke,
                opacity,
                stroke_width,
                reply,
            })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P3-ANN-004 — add a polygon / polyline. Await-holding for tests.
    #[allow(clippy::too_many_arguments)]
    pub async fn add_polygon(
        &self,
        page: i32,
        closed: bool,
        points: Vec<[f32; 2]>,
        stroke: String,
        fill: Option<String>,
        opacity: f32,
        stroke_width: f32,
    ) -> Result<HistoryState, CommandError> {
        let rx = self.add_polygon_request(page, closed, points, stroke, fill, opacity, stroke_width)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_polygon_request(
        &self,
        page: i32,
        closed: bool,
        points: Vec<[f32; 2]>,
        stroke: String,
        fill: Option<String>,
        opacity: f32,
        stroke_width: f32,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::AddPolygon {
                page,
                closed,
                points,
                stroke,
                fill,
                opacity,
                stroke_width,
                reply,
            })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P3-ANN-005 — add a freehand ink stroke. Await-holding for tests.
    pub async fn add_ink(
        &self,
        page: i32,
        points: Vec<[f32; 3]>,
        color: String,
        opacity: f32,
        base_width: f32,
    ) -> Result<HistoryState, CommandError> {
        let rx = self.add_ink_request(page, points, color, opacity, base_width)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn add_ink_request(
        &self,
        page: i32,
        points: Vec<[f32; 3]>,
        color: String,
        opacity: f32,
        base_width: f32,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::AddInk { page, points, color, opacity, base_width, reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P3-ANN-006 — add a stamp. Await-holding for tests.
    pub async fn add_stamp(
        &self,
        page: i32,
        rect: [f32; 4],
        text: String,
        name: String,
        color: String,
        opacity: f32,
    ) -> Result<HistoryState, CommandError> {
        let rx = self.add_stamp_request(page, rect, text, name, color, opacity)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn add_stamp_request(
        &self,
        page: i32,
        rect: [f32; 4],
        text: String,
        name: String,
        color: String,
        opacity: f32,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::AddStamp { page, rect, text, name, color, opacity, reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P3-ANN-006 (P3.C3b) — add an image stamp from PNG `image` bytes.
    /// Await-holding for tests.
    #[allow(clippy::too_many_arguments)]
    pub async fn add_image_stamp(
        &self,
        page: i32,
        x: f32,
        y: f32,
        height: f32,
        image: Vec<u8>,
        text: Option<String>,
        opacity: f32,
    ) -> Result<HistoryState, CommandError> {
        let rx = self.add_image_stamp_request(page, x, y, height, image, text, opacity)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_image_stamp_request(
        &self,
        page: i32,
        x: f32,
        y: f32,
        height: f32,
        image: Vec<u8>,
        text: Option<String>,
        opacity: f32,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::AddImageStamp { page, x, y, height, image, text, opacity, reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P3-ANN-007 — add a measurement. Await-holding for tests.
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    pub async fn add_measure(
        &self,
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
        let rx = self.add_measure_request(
            page, kind, points, color, label, opacity, stroke_width, units_per_point, unit,
        )?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_measure_request(
        &self,
        page: i32,
        kind: String,
        points: Vec<[f32; 2]>,
        color: String,
        label: String,
        opacity: f32,
        stroke_width: f32,
        units_per_point: f32,
        unit: String,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::AddMeasure {
                page,
                kind,
                points,
                color,
                label,
                opacity,
                stroke_width,
                units_per_point,
                unit,
                reply,
            })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P4-EDIT-001 (P4.A1) — extract a page's text runs. Await-holding
    /// convenience for tests; IPC uses `read_text_runs_request`.
    pub async fn read_text_runs(&self, page: usize) -> Result<Vec<TextRun>, CommandError> {
        let rx = self.read_text_runs_request(page)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn read_text_runs_request(
        &self,
        page: usize,
    ) -> Result<oneshot::Receiver<Result<Vec<TextRun>, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::ReadTextRuns { page, reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P4-EDIT-002 (P4.A2) — resolve the document's fonts. Await-holding
    /// convenience for tests; IPC uses `read_font_report_request`.
    pub async fn read_font_report(&self) -> Result<FontReport, CommandError> {
        let rx = self.read_font_report_request()?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn read_font_report_request(
        &self,
    ) -> Result<oneshot::Receiver<Result<FontReport, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::ReadFontReport { reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P4-EDIT-001 (P4.B1) — replace a text run. Await-holding convenience
    /// for tests; IPC uses `replace_text_run_request`.
    pub async fn replace_text_run(
        &self,
        page: usize,
        run_index: usize,
        new_text: String,
    ) -> Result<HistoryState, CommandError> {
        let rx = self.replace_text_run_request(page, run_index, new_text)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn replace_text_run_request(
        &self,
        page: usize,
        run_index: usize,
        new_text: String,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::ReplaceTextRun {
                page,
                run_index,
                new_text,
                reply,
            })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P4-EDIT-004 (P4.B3) — delete a text run. Await-holding for tests; IPC
    /// uses `delete_text_run_request`.
    pub async fn delete_text_run(
        &self,
        page: usize,
        run_index: usize,
    ) -> Result<HistoryState, CommandError> {
        let rx = self.delete_text_run_request(page, run_index)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn delete_text_run_request(
        &self,
        page: usize,
        run_index: usize,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::DeleteTextRun {
                page,
                run_index,
                reply,
            })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P4-EDIT-006 (P4.C2) — locate images on a page. Await-holding for tests.
    pub async fn read_images(&self, page: usize) -> Result<Vec<ImageInfo>, CommandError> {
        let rx = self.read_images_request(page)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn read_images_request(
        &self,
        page: usize,
    ) -> Result<oneshot::Receiver<Result<Vec<ImageInfo>, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::ReadImages { page, reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P4-EDIT-006 (P4.C2) — move/resize/rotate an image. Await-holding for tests.
    pub async fn transform_image(
        &self,
        page: usize,
        index: usize,
        matrix: [f32; 6],
    ) -> Result<HistoryState, CommandError> {
        let rx = self.transform_image_request(page, index, matrix)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn transform_image_request(
        &self,
        page: usize,
        index: usize,
        matrix: [f32; 6],
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::TransformImage {
                page,
                index,
                matrix,
                reply,
            })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P4-EDIT-006 (P4.C2) — delete an image. Await-holding for tests.
    pub async fn delete_image(
        &self,
        page: usize,
        index: usize,
    ) -> Result<HistoryState, CommandError> {
        let rx = self.delete_image_request(page, index)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn delete_image_request(
        &self,
        page: usize,
        index: usize,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::DeleteImage { page, index, reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P4-EDIT-006 (P4.C2b) — replace an image's pixels. Await-holding for tests.
    pub async fn replace_image(
        &self,
        page: usize,
        index: usize,
        image: Vec<u8>,
    ) -> Result<HistoryState, CommandError> {
        let rx = self.replace_image_request(page, index, image)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn replace_image_request(
        &self,
        page: usize,
        index: usize,
        image: Vec<u8>,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::ReplaceImage {
                page,
                index,
                image,
                reply,
            })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P3-ANN-007 (P3.C4b) — read the document's measurement calibration.
    /// Await-holding convenience for tests; IPC uses the `_request` form.
    pub async fn read_measure_calibration(&self) -> Result<Option<MeasureCalibration>, CommandError> {
        let rx = self.read_measure_calibration_request()?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn read_measure_calibration_request(
        &self,
    ) -> Result<oneshot::Receiver<Result<Option<MeasureCalibration>, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::ReadMeasureCalibration { reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P2-PAGE-003 — delete `pages` (0-based indices). Await-holding
    /// convenience for tests; IPC uses `delete_pages_request`.
    pub async fn delete_pages(&self, pages: Vec<i32>) -> Result<HistoryState, CommandError> {
        let rx = self.delete_pages_request(pages)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn delete_pages_request(
        &self,
        pages: Vec<i32>,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::DeletePages { pages, reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P2-PAGE-004 — insert a blank page at `index`. Await-holding
    /// convenience for tests; IPC uses `insert_blank_page_request`.
    pub async fn insert_blank_page(
        &self,
        index: i32,
        size: Option<(f32, f32)>,
    ) -> Result<HistoryState, CommandError> {
        let rx = self.insert_blank_page_request(index, size)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn insert_blank_page_request(
        &self,
        index: i32,
        size: Option<(f32, f32)>,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::InsertBlankPage { index, size, reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P2-PAGE-009 — crop `page` to `rect`, or reset when `None`.
    /// Await-holding convenience for tests; IPC uses `crop_page_request`.
    pub async fn crop_page(
        &self,
        page: i32,
        rect: Option<(f32, f32, f32, f32)>,
    ) -> Result<HistoryState, CommandError> {
        let rx = self.crop_page_request(page, rect)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn crop_page_request(
        &self,
        page: i32,
        rect: Option<(f32, f32, f32, f32)>,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::CropPage { page, rect, reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P2-PAGE-002 — reorder pages by the given permutation.
    /// Await-holding convenience for tests; IPC uses `reorder_pages_request`.
    pub async fn reorder_pages(&self, order: Vec<usize>) -> Result<HistoryState, CommandError> {
        let rx = self.reorder_pages_request(order)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn reorder_pages_request(
        &self,
        order: Vec<usize>,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::ReorderPages { order, reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P2-PAGE-005 — insert `pages` of `source_path` at `index`.
    /// Await-holding convenience for tests; IPC uses `insert_from_pdf_request`.
    pub async fn insert_from_pdf(
        &self,
        source_path: PathBuf,
        pages: Vec<i32>,
        index: i32,
    ) -> Result<HistoryState, CommandError> {
        let rx = self.insert_from_pdf_request(source_path, pages, index)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn insert_from_pdf_request(
        &self,
        source_path: PathBuf,
        pages: Vec<i32>,
        index: i32,
    ) -> Result<oneshot::Receiver<Result<HistoryState, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::InsertFromPdf { source_path, pages, index, reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P2-PAGE-006 — extract `pages` into a new PDF at `dest`.
    /// Await-holding convenience for tests; IPC uses `extract_pages_request`.
    pub async fn extract_pages(
        &self,
        pages: Vec<i32>,
        dest: PathBuf,
    ) -> Result<SaveOutcome, CommandError> {
        let rx = self.extract_pages_request(pages, dest)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn extract_pages_request(
        &self,
        pages: Vec<i32>,
        dest: PathBuf,
    ) -> Result<oneshot::Receiver<Result<SaveOutcome, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::ExtractPages { pages, dest, reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// SPEC: P2-PAGE-007 — split into multiple files under `dest_dir`.
    /// Await-holding convenience for tests; IPC uses `split_document_request`.
    pub async fn split_document(
        &self,
        mode: SplitMode,
        dest_dir: PathBuf,
        stem: String,
    ) -> Result<SplitOutcome, CommandError> {
        let rx = self.split_document_request(mode, dest_dir, stem)?;
        rx.await
            .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
    }

    pub fn split_document_request(
        &self,
        mode: SplitMode,
        dest_dir: PathBuf,
        stem: String,
    ) -> Result<oneshot::Receiver<Result<SplitOutcome, CommandError>>, CommandError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Message::SplitDocument { mode, dest_dir, stem, reply })
            .map_err(|_| CommandError::Internal("doc-actor mailbox closed".into()))?;
        Ok(rx)
    }

    /// Best-effort graceful close. Sends `Close`; the worker exits on
    /// next iteration. Dropping the handle has the same effect (mailbox
    /// closes) so callers that don't care can just `drop(handle)`.
    pub fn close(&self) {
        // If the worker already exited, the send fails — that's fine.
        let _ = self.tx.send(Message::Close);
    }
}

impl Drop for DocumentActorHandle {
    fn drop(&mut self) {
        // Sender drop closes the mailbox, which terminates the worker's
        // `for msg in rx` loop with `RecvError`. We do not join the
        // thread: nothing here should ever wait on the worker, and the
        // OS reaps the thread on exit.
        tracing::trace!(doc_id = %self.id, "doc-actor handle dropped");
    }
}

/// Worker thread body. Opens the PDF, signals readiness through
/// `ready`, then services messages until the mailbox closes or a
/// `Close` arrives.
///
/// Args are passed by value because this function is called from
/// inside a `move` closure on a freshly spawned `std::thread`, which
/// requires owned values — there is no caller frame to borrow from.
// The worker is one big message-dispatch loop; its length is the sum of
// the per-message arms, not incidental complexity. Splitting the arms
// into free functions would mean threading `doc`/`dirty`/`history`/
// `autosave_dir` through each — more noise than the lint saves.
#[allow(clippy::needless_pass_by_value, clippy::too_many_lines)]
fn run_worker(
    app: Option<AppHandle>,
    id: Uuid,
    path: PathBuf,
    password: Option<String>,
    rx: mpsc::Receiver<Message>,
    ready: mpsc::Sender<Result<DocumentMetadata, CommandError>>,
) {
    let span = tracing::info_span!("doc_actor", doc_id = %id, path = %path.display());
    let _enter = span.enter();

    let pwd_ref = password.as_deref();
    let (mut doc, metadata) = match open_pdf(&path, pwd_ref) {
        Ok(pair) => pair,
        Err(e) => {
            // Tell `spawn()` that open failed; it surfaces the typed
            // error to the IPC caller. Then exit the thread cleanly.
            let _ = ready.send(Err(e));
            tracing::info!("doc-actor exiting (open failed)");
            return;
        }
    };

    tracing::info!(page_count = metadata.page_count, "doc-actor started");
    if ready.send(Ok(metadata.clone())).is_err() {
        // Caller went away before we could report success; nothing to
        // serve, exit.
        tracing::warn!("doc-actor exiting (caller dropped ready channel)");
        return;
    }

    emit_change(
        app.as_ref(),
        &DocumentChange::Opened {
            id: id.to_string(),
            page_count: metadata.page_count,
        },
    );

    // SPEC: P2-SAVE-001 / P2.A2 — the document is dirty whenever the undo
    // history's live state id differs from the id we last persisted. This
    // is derived (not a standalone flag) so undo-to-saved, redo, forked
    // history, and depth-cap eviction all report correctly — see
    // `UndoStack::current_state_id` and FABLE_REVIEW §3.11 (P4.HF12).
    // `0` is the pristine (as-opened) state, which is what is on disk.
    let mut saved_state_id: u64 = 0;

    // SPEC: P2-PAGE-003 / session history — per-document undo/redo. Empty
    // in P2.A3 (no page operations exist yet to record onto it); the
    // P2.B* steps push an inverse on every mutating message. Inference
    // pins the target type to `doc`'s on first `undo`/`redo` call.
    let mut history = UndoStack::new();

    // SPEC: NFR-PERF-005 — cache the parsed `lopdf::Document` so a post-edit
    // burst of reads (annotations panel, text boxes, free-text, notes, measure,
    // across every visible page) shares one parse instead of re-parsing the whole
    // document per query. Invalidated below whenever an edit changes the doc.
    let mut doc_cache = CachedDoc::new();

    // SPEC: P2.A2 — where this document's autosave/recovery copy lives.
    // Derived from the AppHandle, so it is `None` under `cargo test`
    // (app = None) → autosave and its cleanup are no-ops there.
    let id_str = id.to_string();
    let autosave_dir = app.as_ref().and_then(|a| autosave::autosave_dir(a).ok());

    while let Ok(msg) = rx.recv() {
        // Any mutation records an undo inverse, so a changed state id after the
        // match means the document changed → drop the read cache (next read
        // re-parses). Reads leave history untouched, so the cache stays warm.
        let state_before = history.current_state_id();
        match msg {
            Message::GetPageCount { reply } => {
                let _ = reply.send(metadata.page_count);
            }
            Message::GetMetadata { reply } => {
                // Re-read so a future write op that changes the
                // metadata is reflected. For B1 this is identical to
                // the cached copy.
                let _ = reply.send(collect_metadata(&doc));
            }
            Message::RenderThumbnail {
                page,
                max_width,
                reply,
            } => {
                let _ = reply.send(render::render_thumbnail(&doc, page, max_width));
            }
            Message::RenderPage {
                page,
                dpi,
                format,
                reply,
            } => {
                let _ = reply.send(render::render_page(&doc, page, dpi, format));
            }
            Message::ReadTextRuns { page, reply } => {
                // SPEC: P4-EDIT-001 (P4.A1) — read-only; extract_text_runs holds
                // the PDFium lock itself (same as render_page).
                let _ = reply.send(extract_text_runs(&doc, page));
            }
            Message::ReadFontReport { reply } => {
                // SPEC: P4-EDIT-002 (P4.A2) — read-only. collect_document_fonts
                // holds the PDFium lock; build_font_report is pure (+ a one-time
                // OS font-dir scan).
                let result = collect_document_fonts(&doc).map(build_font_report);
                let _ = reply.send(result);
            }
            Message::ReplaceTextRun {
                page,
                run_index,
                new_text,
                reply,
            } => {
                // SPEC: P4-EDIT-001 (P4.B1) — edit a run's text in place; the
                // inverse is a pre-edit byte snapshot (RestoreDocEdit).
                let edit = ReplaceTextRunEdit {
                    page,
                    run_index,
                    new_text,
                };
                let result = match Box::new(edit).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::DeleteTextRun {
                page,
                run_index,
                reply,
            } => {
                // SPEC: P4-EDIT-004 (P4.B3) — remove a run via lopdf; the inverse is
                // a pre-delete byte snapshot (RestoreDocEdit).
                let edit = DeleteTextRunEdit { page, run_index };
                let result = match Box::new(edit).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::ReadImages { page, reply } => {
                // SPEC: P4-EDIT-006 (P4.C2) — read-only; extract_images holds the
                // PDFium lock itself (like render_page).
                let _ = reply.send(extract_images(&doc, page));
            }
            Message::TransformImage {
                page,
                index,
                matrix,
                reply,
            } => {
                // SPEC: P4-EDIT-006 (P4.C2) — move/resize/rotate; inverse is a
                // pre-edit byte snapshot (RestoreDocEdit).
                let edit = TransformImageEdit { page, index, matrix };
                let result = match Box::new(edit).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::DeleteImage { page, index, reply } => {
                // SPEC: P4-EDIT-006 (P4.C2) — delete via lopdf splice; inverse is a
                // pre-delete byte snapshot (RestoreDocEdit).
                let edit = DeleteImageEdit { page, index };
                let result = match Box::new(edit).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::ReplaceImage {
                page,
                index,
                image,
                reply,
            } => {
                // SPEC: P4-EDIT-006 (P4.C2b) — swap the image's pixels; inverse is a
                // pre-edit byte snapshot (RestoreDocEdit).
                let edit = ReplaceImageEdit { page, index, image };
                let result = match Box::new(edit).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::Save {
                path: dest_arg,
                reply,
            } => {
                // SPEC: P2-SAVE-001 — a same-path save with no unsaved
                // changes is a *true* no-op: the user's file is never
                // rewritten, so its bytes (and hash) stay identical. A
                // save-as (an explicit `dest_arg`) always writes.
                let dest = dest_arg.unwrap_or_else(|| path.clone());
                let same_path = dest == path;
                let clean = history.current_state_id() == saved_state_id;
                let result = if same_path && clean {
                    Ok(SaveOutcome {
                        path: dest.to_string_lossy().into_owned(),
                        bytes_written: 0,
                        no_op: true,
                    })
                } else {
                    // make_backup only when overwriting the original; a
                    // same-path save that reaches here is, by the branch
                    // above, necessarily dirty.
                    let outcome = save_document(&doc, &dest, same_path, password.as_deref());
                    if outcome.is_ok() {
                        // Any successful save — same-path *or* save-as — makes
                        // the in-memory document clean: record the state we
                        // just persisted and drop any recovery copy, since
                        // there are no unsaved changes left to recover. Doing
                        // this for save-as too is what fixes the false
                        // "unsaved changes" claim. SPEC: P2-SAVE-001 / P2.A2;
                        // FABLE_REVIEW §3.11 (P4.HF12).
                        saved_state_id = history.current_state_id();
                        if let Some(dir) = autosave_dir.as_deref() {
                            let _ = autosave::discard_autosave(dir, &id_str);
                        }
                    }
                    outcome
                };
                let _ = reply.send(result);
            }
            Message::Undo { reply } => {
                // Dirtiness is derived from the history's live state id, so
                // undo/redo need no bookkeeping here: undoing back to the
                // saved state reports clean, an empty-stack undo is a
                // harmless no-op that leaves the id unchanged.
                let result = history.undo(&mut doc);
                let _ = reply.send(result);
            }
            Message::Redo { reply } => {
                let result = history.redo(&mut doc);
                let _ = reply.send(result);
            }
            Message::GetHistoryState { reply } => {
                let _ = reply.send(history.state());
            }
            Message::GetBytes { reply } => {
                // Serialize under the shared PDFium lock (FX_GE global
                // state); same path the explicit save uses.
                let result = pdfium_lock().and_then(|_guard| {
                    doc.save_to_bytes().map_err(CommandError::from)
                });
                let _ = reply.send(result);
            }
            Message::RotatePages {
                pages,
                quarter_turns,
                reply,
            } => {
                // SPEC: P2-PAGE-001 — the first real Edit<PdfDocument>.
                // apply() mutates the doc and hands back the inverse; on
                // success record it for undo and mark the doc dirty.
                let result = match Box::new(RotateEdit {
                    pages,
                    quarter_turns,
                })
                .apply(&mut doc)
                {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::DeletePages { pages, reply } => {
                // SPEC: P2-PAGE-003 — same edit/undo dance as rotate.
                let result = match Box::new(DeleteEdit { pages }).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::InsertBlankPage { index, size, reply } => {
                // SPEC: P2-PAGE-004 — insert; inverse is a delete.
                let result = match Box::new(InsertBlankEdit { index, size }).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::CropPage { page, rect, reply } => {
                // SPEC: P2-PAGE-009 — adjust /CropBox; inverse restores it.
                let result = match Box::new(CropEdit { page, rect }).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::ReorderPages { order, reply } => {
                // SPEC: P2-PAGE-002 — reorder via the lopdf COS layer; this
                // replaces the live document with the reordered bytes. Records
                // the inverse permutation and marks the document dirty.
                let result = match Box::new(ReorderEdit { order }).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::ResizePages {
                pages,
                width,
                height,
                preserve_aspect,
                reply,
            } => {
                // SPEC: P2-PAGE-010 — scale content + set MediaBox; the inverse
                // is a pre-resize byte snapshot (RestoreDocEdit).
                let edit = ResizeEdit {
                    pages,
                    width,
                    height,
                    preserve_aspect,
                };
                let result = match Box::new(edit).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::AddTextMarkup {
                page,
                subtype,
                quads,
                color,
                opacity,
                reply,
            } => {
                // SPEC: P3-ANN-001 — write the markup annotation via lopdf; the
                // inverse is a pre-write byte snapshot (RestoreDocEdit).
                let edit = TextMarkupEdit {
                    page,
                    subtype,
                    quads,
                    color,
                    opacity,
                };
                let result = match Box::new(edit).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::ClearTextMarkup { reply } => {
                // SPEC: P3-ANN-001 — strip all markup; snapshot inverse.
                let result = match Box::new(ClearMarkupEdit).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::AddNote { note_id, page, x, y, content, author, reply } => {
                // SPEC: P3-ANN-002 — add a /Text note; snapshot inverse.
                let edit = AddNoteEdit { note_id, page, x, y, content, author };
                let result = match Box::new(edit).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::AddReply { parent_id, author, content, reply } => {
                // SPEC: P3-ANN-009 — a /Text linked via /IRT; snapshot inverse.
                let edit = ReplyEdit { parent_id, author, content };
                let result = match Box::new(edit).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::UpdateNote { note_id, content, reply } => {
                let edit = UpdateNoteEdit { note_id, content };
                let result = match Box::new(edit).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::DeleteAnnotation { note_id, reply } => {
                let edit = DeleteAnnotationEdit { note_id };
                let result = match Box::new(edit).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::ReadNotes { reply } => {
                // Read-only. SPEC: NFR-PERF-005 — serve from the shared parse
                // cache; only a cold cache serializes (under the PDFium lock, the
                // GetBytes path) + parses, so a post-edit read burst parses once.
                let result = doc_cache
                    .get(|| {
                        let _guard = pdfium_lock()?;
                        doc.save_to_bytes().map_err(CommandError::from)
                    })
                    .and_then(read_text_notes_doc);
                let _ = reply.send(result);
            }
            Message::ReadAnnotations { reply } => {
                // Read-only. SPEC: NFR-PERF-005 — served from the shared parse cache.
                let result = doc_cache
                    .get(|| {
                        let _guard = pdfium_lock()?;
                        doc.save_to_bytes().map_err(CommandError::from)
                    })
                    .and_then(read_annotations_doc);
                let _ = reply.send(result);
            }
            Message::ExportAnnotations { dest, reply } => {
                // SPEC: P3-ANN-010 — read-only: serialize the live doc, build the
                // XFDF, write the sidecar file. No undo, no dirty change.
                let result = pdfium_lock()
                    .and_then(|_guard| doc.save_to_bytes().map_err(CommandError::from))
                    .and_then(|bytes| annotations_to_xfdf(&bytes))
                    .and_then(|(xml, count)| {
                        std::fs::write(&dest, xml)
                            .map_err(|e| CommandError::Internal(format!("write xfdf: {e}")))?;
                        Ok(count)
                    });
                let _ = reply.send(result);
            }
            Message::ImportXfdf { xfdf, reply } => {
                // SPEC: P3-ANN-010 — apply the XFDF as one undoable edit; the
                // inverse is a pre-import byte snapshot (RestoreDocEdit).
                let result = match Box::new(ImportXfdfEdit { xfdf }).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::FlattenAnnotations { reply } => {
                // SPEC: P3-ANN-011 — bake annotations into the page content; the
                // inverse is a pre-flatten byte snapshot (in-session undo only).
                let result = match Box::new(FlattenEdit).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::ReadFreeText { nm, reply } => {
                // Read-only. SPEC: NFR-PERF-005 — served from the shared parse cache.
                let result = doc_cache
                    .get(|| {
                        let _guard = pdfium_lock()?;
                        doc.save_to_bytes().map_err(CommandError::from)
                    })
                    .and_then(|d| read_free_text_doc(d, &nm));
                let _ = reply.send(result);
            }
            Message::UpdateFreeText {
                nm,
                text,
                font_family,
                font_size,
                color,
                bold,
                italic,
                underline,
                reply,
            } => {
                let edit = UpdateFreeTextEdit {
                    nm,
                    text,
                    font_family,
                    font_size,
                    color,
                    bold,
                    italic,
                    underline,
                };
                let result = match Box::new(edit).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::AddFreeText {
                page,
                rect,
                text,
                font_family,
                font_size,
                color,
                bold,
                italic,
                underline,
                reply,
            } => {
                let edit = FreeTextEdit {
                    page,
                    rect,
                    text,
                    font_family,
                    font_size,
                    color,
                    bold,
                    italic,
                    underline,
                };
                let result = match Box::new(edit).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::AddTextBox {
                page,
                rect,
                text,
                font_family,
                font_size,
                color,
                bold,
                italic,
                underline,
                reply,
            } => {
                // SPEC: P4-EDIT-003 (P4.B2) — text into the page content stream; the
                // inverse is a pre-write byte snapshot (RestoreDocEdit).
                let edit = TextBoxEdit {
                    page,
                    rect,
                    text,
                    font_family,
                    font_size,
                    color,
                    bold,
                    italic,
                    underline,
                };
                let result = match Box::new(edit).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::UpdateTextBox {
                page,
                id,
                text,
                font_family,
                font_size,
                color,
                bold,
                italic,
                underline,
                reply,
            } => {
                // SPEC: P4-EDIT-003b — re-edit a text box in place; the inverse is a
                // pre-write byte snapshot (RestoreDocEdit).
                let edit = UpdateTextBoxEdit {
                    page,
                    id,
                    text,
                    font_family,
                    font_size,
                    color,
                    bold,
                    italic,
                    underline,
                };
                let result = match Box::new(edit).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::ReadTextBoxes { page, reply } => {
                // SPEC: P4-EDIT-003b — read-only. NFR-PERF-005: served from the
                // shared parse cache (cold cache serializes under the PDFium lock).
                let result = doc_cache
                    .get(|| {
                        let _guard = pdfium_lock()?;
                        doc.save_to_bytes().map_err(CommandError::from)
                    })
                    .and_then(|d| read_text_boxes_doc(d, page));
                let _ = reply.send(result);
            }
            Message::RemoveTextBox { page, id, reply } => {
                // SPEC: P4-EDIT-003b / P4-EDIT-004 — delete a text box; the inverse is
                // a pre-write byte snapshot (RestoreDocEdit).
                let edit = RemoveTextBoxEdit { page, id };
                let result = match Box::new(edit).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::AddImage {
                page,
                rect,
                image,
                reply,
            } => {
                // SPEC: P4-EDIT-005 (P4.C1) — image into the page content stream; the
                // inverse is a pre-write byte snapshot (RestoreDocEdit).
                let edit = AddImageEdit { page, rect, image };
                let result = match Box::new(edit).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::AddLink {
                page,
                rect,
                kind,
                value,
                style,
                color,
                reply,
            } => {
                // SPEC: P4-EDIT-007 (P4.C3) / P4-EDIT-007b — /Link annotation + its
                // appearance; the inverse is a pre-write byte snapshot (RestoreDocEdit).
                let edit = AddLinkEdit { page, rect, kind, value, style, color };
                let result = match Box::new(edit).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::AddWatermark {
                pages,
                kind,
                opacity,
                rotation,
                behind,
                reply,
            } => {
                // SPEC: P4-EDIT-009 (P4.D2) — watermark as page content; the inverse
                // is a pre-write byte snapshot (RestoreDocEdit).
                let edit = WatermarkEdit { pages, kind, opacity, rotation, behind };
                let result = match Box::new(edit).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::RemoveWatermarks { reply } => {
                // SPEC: P4-EDIT-009 — strip all VibePDF watermarks; the inverse is a
                // pre-write byte snapshot (RestoreDocEdit).
                let result = match Box::new(RemoveWatermarksEdit).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::AddBackground {
                pages,
                kind,
                opacity,
                reply,
            } => {
                // SPEC: P4-EDIT-008 (P4.D1) — background as prepended page content; the
                // inverse is a pre-write byte snapshot (RestoreDocEdit).
                let edit = BackgroundEdit { pages, kind, opacity };
                let result = match Box::new(edit).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::AddHeaderFooter {
                pages,
                position,
                left,
                center,
                right,
                font_family,
                size,
                color,
                margin,
                date,
                reply,
            } => {
                // SPEC: P4-EDIT-010 (P4.D3) — header/footer appended as page content;
                // the inverse is a pre-write byte snapshot (RestoreDocEdit).
                let edit = HeaderFooterEdit {
                    pages,
                    position,
                    left,
                    center,
                    right,
                    font_family,
                    size,
                    color,
                    margin,
                    date,
                };
                let result = match Box::new(edit).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::AddShape {
                page,
                kind,
                rect,
                stroke,
                fill,
                opacity,
                stroke_width,
                reply,
            } => {
                let edit = ShapeEdit { page, kind, rect, stroke, fill, opacity, stroke_width };
                let result = match Box::new(edit).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::AddLine { page, x1, y1, x2, y2, arrow, stroke, opacity, stroke_width, reply } => {
                let edit = LineEdit { page, x1, y1, x2, y2, arrow, stroke, opacity, stroke_width };
                let result = match Box::new(edit).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::AddPolygon { page, closed, points, stroke, fill, opacity, stroke_width, reply } => {
                let edit = PolygonEdit { page, closed, points, stroke, fill, opacity, stroke_width };
                let result = match Box::new(edit).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::AddInk { page, points, color, opacity, base_width, reply } => {
                let edit = InkEdit { page, points, color, opacity, base_width };
                let result = match Box::new(edit).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::AddStamp { page, rect, text, name, color, opacity, reply } => {
                let edit = StampEdit { page, rect, text, name, color, opacity };
                let result = match Box::new(edit).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::AddImageStamp { page, x, y, height, image, text, opacity, reply } => {
                let edit = ImageStampEdit { page, x, y, height, image, text, opacity };
                let result = match Box::new(edit).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::AddMeasure {
                page,
                kind,
                points,
                color,
                label,
                opacity,
                stroke_width,
                units_per_point,
                unit,
                reply,
            } => {
                let edit = MeasureEdit {
                    page,
                    kind,
                    points,
                    color,
                    label,
                    opacity,
                    stroke_width,
                    units_per_point,
                    unit,
                };
                let result = match Box::new(edit).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::ReadMeasureCalibration { reply } => {
                // SPEC: P3-ANN-007 (P3.C4b) — read-only. NFR-PERF-005: served from
                // the shared parse cache (cold cache serializes under the lock).
                let result = doc_cache
                    .get(|| {
                        let _guard = pdfium_lock()?;
                        doc.save_to_bytes().map_err(CommandError::from)
                    })
                    .and_then(read_measure_calibration_doc);
                let _ = reply.send(result);
            }
            Message::InsertFromPdf { source_path, pages, index, reply } => {
                // SPEC: P2-PAGE-005 — import pages from another file; the edit
                // opens the source itself. Records its inverse (a delete of the
                // inserted block) and marks the document dirty.
                let edit = InsertFromEdit { source_path, pages, index };
                let result = match Box::new(edit).apply(&mut doc) {
                    Ok(inverse) => {
                        history.record(inverse);
                        Ok(history.state())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            Message::ExtractPages { pages, dest, reply } => {
                // SPEC: P2-PAGE-006 — read-only: build a new PDF from the
                // source pages. No undo, no dirty (the open doc is unchanged).
                let _ = reply.send(extract_pages(&doc, pages, &dest));
            }
            Message::SplitDocument { mode, dest_dir, stem, reply } => {
                // SPEC: P2-PAGE-007 — read-only: emit N files from the source.
                // No undo, no dirty (the open doc is unchanged).
                let _ = reply.send(split_document(&doc, &mode, &dest_dir, &stem));
            }
            Message::Autosave => {
                // SPEC: P2.A2 — write a recovery copy only when dirty, i.e.
                // the live history state differs from the last save.
                // Best-effort: failures are logged, never fatal.
                if history.current_state_id() != saved_state_id {
                    if let Some(dir) = autosave_dir.as_deref() {
                        match autosave::write_autosave(
                            &doc,
                            dir,
                            &id_str,
                            &path.to_string_lossy(),
                        ) {
                            Ok(p) => {
                                tracing::info!(autosave = %p.display(), "autosaved dirty document");
                            }
                            Err(e) => tracing::warn!(error = %e, "autosave failed"),
                        }
                    }
                }
            }
            Message::Close => {
                tracing::info!("doc-actor closing (Close received)");
                break;
            }
        }

        // SPEC: NFR-PERF-005 — a mutation records undo history, so a changed
        // state id after handling the message means the document changed → drop
        // the read cache (next read re-parses the edited bytes). Reads leave
        // history untouched, so the cache survives the post-edit read burst.
        if history.current_state_id() != state_before {
            doc_cache.invalidate();
        }
    }

    // SPEC: P2.A2 — a graceful exit (an explicit `Close`, or the mailbox
    // closing once every handle drops) means there was no crash, so no
    // recovery copy should linger. A real crash never reaches here, so
    // its autosave survives to be offered on the next launch.
    if let Some(dir) = autosave_dir.as_deref() {
        let _ = autosave::discard_autosave(dir, &id_str);
    }

    // Close the document under the shared PDFium lock — FPDF_CloseDocument
    // (PdfDocument's Drop) touches global state and must not race another
    // actor's render/save/rotate. `.ok()` because a poisoned lock is no
    // reason to leak the document; we still drop it.
    let close_guard = pdfium_lock().ok();
    drop(doc);
    drop(close_guard);

    emit_change(
        app.as_ref(),
        &DocumentChange::Closed { id: id.to_string() },
    );
    tracing::info!("doc-actor exited");
}

fn emit_change(app: Option<&AppHandle>, change: &DocumentChange) {
    if let Some(app) = app {
        if let Err(e) = app.emit(DOCUMENT_CHANGED_EVENT, change) {
            tracing::warn!(error = %e, "failed to emit document-changed");
        }
    }
}
