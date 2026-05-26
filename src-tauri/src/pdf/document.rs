use std::path::Path;
use std::sync::LazyLock;

use pdfium_render::prelude::*;

use crate::error::CommandError;

/// Metadata snapshot taken once when the document is opened. The actor
/// caches this so cheap queries (page count, title) don't have to round-
/// trip into `PDFium` on every call.
///
/// Held by both the actor (canonical copy) and `OpenedDocument` (wire
/// copy). Extending this struct is fine; renaming fields is a breaking
/// IPC change — keep the camelCase serde rename stable.
#[derive(Clone, Debug, Default)]
pub struct DocumentMetadata {
    pub page_count: u32,
    pub title: Option<String>,
    pub author: Option<String>,
    pub pdf_version: Option<String>,
}

/// Shared `PDFium` instance.
///
/// `PDFium` is single-threaded per *document*, but the library itself is
/// safe to share read-only across threads once bound. We bind once via
/// `LazyLock` — `OnceLock` is not enough here because the fallible
/// initializer races: two callers can both call `bind_to_system_library`
/// before either `set`s, and `PDFium`'s global `FPDF_InitLibrary` is not
/// re-entrant (the loser's `Drop` will tear down the library while the
/// winner is still using it → SIGTRAP under `cargo test`'s default
/// parallel runner). `LazyLock` guarantees exactly one init.
///
/// The init result is cached as `Result<Pdfium, String>` because
/// `PdfiumError` isn't `Clone`. Per-call we re-wrap the cached string
/// into a fresh `CommandError`.
static PDFIUM: LazyLock<Result<Pdfium, String>> = LazyLock::new(|| {
    // pdfium-render 0.9 exposes `bind_to_system_library`; the static
    // binding option lives behind a non-default cargo feature we don't
    // depend on. `bind_to_system_library` walks the standard search
    // path (incl. src-tauri/resources/pdfium/ at dev time).
    Pdfium::bind_to_system_library()
        .map(Pdfium::new)
        .map_err(|e| format!("could not load PDFium: {e}. Run `npm run fetch-pdfium`."))
});

pub fn pdfium() -> Result<&'static Pdfium, CommandError> {
    PDFIUM.as_ref().map_err(|e| CommandError::Internal(e.clone()))
}

/// Open a document and return both the live handle and a metadata
/// snapshot. The returned `PdfDocument<'static>` borrows from the global
/// `Pdfium`, which lives forever, so the lifetime is `'static` for our
/// purposes — the document is owned by the actor thread.
///
/// SPEC: P1-VIEW-001 (open) and P1-VIEW-003 (encrypted PDFs) flow through
/// this entry point. B2 wires the password retry-loop UI; for now any
/// caller may pass `None` and observe a `PdfError` on encrypted files.
pub fn open_pdf<'a>(
    path: &Path,
    password: Option<&str>,
) -> Result<(PdfDocument<'a>, DocumentMetadata), CommandError> {
    let p = pdfium()?;
    let doc = p.load_pdf_from_file(path, password).map_err(CommandError::from)?;
    let metadata = collect_metadata(&doc);
    Ok((doc, metadata))
}

/// Read the metadata fields we surface to the frontend. Errors during
/// metadata extraction are non-fatal — the user can still view the doc.
#[must_use]
pub fn collect_metadata(doc: &PdfDocument<'_>) -> DocumentMetadata {
    let page_count = u32::try_from(doc.pages().len()).unwrap_or(u32::MAX);
    let m = doc.metadata();
    let title = m.get(PdfDocumentMetadataTagType::Title).map(|t| t.value().to_string());
    let author = m.get(PdfDocumentMetadataTagType::Author).map(|t| t.value().to_string());
    let pdf_version = Some(format!("{:?}", doc.version()));
    DocumentMetadata {
        page_count,
        title,
        author,
        pdf_version,
    }
}

/// Lightweight metadata-only open used by the smoke test in
/// `tests/pdfium_init.rs` — drops the document immediately after
/// reading the page count. Production callers should go through the
/// actor; see `open_pdf` above.
pub fn open_document_metadata(path: &Path) -> Result<DocumentMetadata, CommandError> {
    let (doc, meta) = open_pdf(path, None)?;
    drop(doc);
    Ok(meta)
}

#[must_use]
pub fn pdfium_version_string() -> String {
    // pdfium-render doesn't expose a runtime version directly; the
    // version is determined by the loaded native lib. For the smoke
    // test we just confirm we can bind.
    match pdfium() {
        Ok(_) => "pdfium: loaded".to_string(),
        Err(e) => format!("pdfium: failed — {e}"),
    }
}
