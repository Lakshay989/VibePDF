use std::path::Path;
use std::sync::OnceLock;

use pdfium_render::prelude::*;

use crate::error::CommandError;

/// Metadata snapshot returned after a successful open. The actor owns the
/// live `PdfDocument`; this struct is what crosses the IPC boundary.
pub struct DocumentMetadata {
    pub page_count: u32,
}

/// Shared PDFium instance.
///
/// PDFium is single-threaded per *document*, but the library itself is
/// safe to share read-only across threads once bound. We bind once and
/// reuse the binding everywhere — see actor.rs for per-document
/// serialization.
fn pdfium() -> Result<&'static Pdfium, CommandError> {
    static INSTANCE: OnceLock<Pdfium> = OnceLock::new();
    if let Some(p) = INSTANCE.get() {
        return Ok(p);
    }
    let bindings = Pdfium::bind_to_statically_linked_library()
        .or_else(|_| {
            // In dev, the prebuilt dylib lives next to the binary or in
            // src-tauri/resources/pdfium/. `bind_to_system_library` walks
            // the standard search path; if that fails we surface a
            // user-actionable error so the bootstrap is obvious.
            Pdfium::bind_to_system_library()
        })
        .map_err(|e| CommandError::Internal(format!(
            "could not load PDFium: {e}. Run `npm run fetch-pdfium`."
        )))?;
    let _ = INSTANCE.set(Pdfium::new(bindings));
    INSTANCE
        .get()
        .ok_or_else(|| CommandError::Internal("PDFium init race".into()))
}

pub fn open_document_metadata(path: &Path) -> Result<DocumentMetadata, CommandError> {
    let p = pdfium()?;
    let doc = p
        .load_pdf_from_file(path, None)
        .map_err(CommandError::from)?;
    let pages = doc.pages();
    Ok(DocumentMetadata {
        page_count: pages.len() as u32,
    })
}

pub fn pdfium_version_string() -> String {
    // pdfium-render doesn't expose a runtime version directly; the
    // version is determined by the loaded native lib. For the smoke
    // test we just confirm we can bind.
    match pdfium() {
        Ok(_) => "pdfium: loaded".to_string(),
        Err(e) => format!("pdfium: failed — {e}"),
    }
}
