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
use crate::pdf::document::{collect_metadata, open_pdf, DocumentMetadata};

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

/// RGBA8 thumbnail; PNG encoding happens in the frontend / D1.
#[derive(Clone, Debug)]
pub struct Thumbnail {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Messages the worker thread accepts. Each variant carries its own
/// reply channel so the worker can answer one message at a time
/// without head-of-line blocking on the mailbox.
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
        reply: oneshot::Sender<Result<Thumbnail, CommandError>>,
    },
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
    ) -> Result<Thumbnail, CommandError> {
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
#[allow(clippy::needless_pass_by_value)]
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
    let (doc, metadata) = match open_pdf(&path, pwd_ref) {
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

    while let Ok(msg) = rx.recv() {
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
                let _ = reply.send(render_thumbnail(&doc, page, max_width));
            }
            Message::Close => {
                tracing::info!("doc-actor closing (Close received)");
                break;
            }
        }
    }

    emit_change(
        app.as_ref(),
        &DocumentChange::Closed { id: id.to_string() },
    );
    tracing::info!("doc-actor exited");
}

/// Render one page to RGBA8 at `max_width` pixels wide, preserving
/// aspect ratio. Used by the thumbnail sidebar (D1) and any future
/// preview surface that wants a small bitmap without paying the cost
/// of PNG encoding inside the actor.
fn render_thumbnail(
    doc: &pdfium_render::prelude::PdfDocument<'_>,
    page: u32,
    max_width: u32,
) -> Result<Thumbnail, CommandError> {
    use pdfium_render::prelude::*;

    let pages = doc.pages();
    // pdfium-render 0.9 uses `PdfPageIndex` (= i32) for page lookup;
    // we reject obvious overflow but otherwise let PDFium return the
    // typed "page out of range" error.
    let page_idx = i32::try_from(page)
        .map_err(|_| CommandError::InvalidInput(format!("page {page} out of range")))?;
    let pdf_page = pages.get(page_idx).map_err(CommandError::from)?;

    let target_w = i32::try_from(max_width.max(1)).unwrap_or(96);
    let config = PdfRenderConfig::new().set_target_width(target_w);

    let bitmap = pdf_page.render_with_config(&config).map_err(CommandError::from)?;
    let width = u32::try_from(bitmap.width()).unwrap_or(0);
    let height = u32::try_from(bitmap.height()).unwrap_or(0);
    let rgba = bitmap.as_rgba_bytes();

    Ok(Thumbnail {
        width,
        height,
        rgba,
    })
}

fn emit_change(app: Option<&AppHandle>, change: &DocumentChange) {
    if let Some(app) = app {
        if let Err(e) = app.emit(DOCUMENT_CHANGED_EVENT, change) {
            tracing::warn!(error = %e, "failed to emit document-changed");
        }
    }
}
