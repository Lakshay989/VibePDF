//! Extract pages to a new PDF (P2.C2).
//!
//! SPEC: P2-PAGE-006 — produce a new PDF containing exactly the selected
//! pages, with resources (fonts, images, color spaces) copied. This is NOT
//! an edit of the source document (it isn't undoable); it builds a fresh
//! document via `FPDF_ImportPages` (`copy_pages_from_document`) and writes
//! it through the verified-save path (P2.A1's `save_document`).

use std::path::Path;

use pdfium_render::prelude::PdfDocument;

use crate::error::CommandError;
use crate::pdf::delete_page::{range_string, validate};
use crate::pdf::document::{pdfium, pdfium_lock, save_document, SaveOutcome};

/// Build a new PDF from `indices` (already-validated, 0-based) of `source`
/// and write it to `dest` (atomic + round-trip-verified). `FPDF_ImportPages`
/// copies the resources each page needs. Shared by extract (P2.C2) and split
/// (P2.C3); callers must validate `indices` against the live page count first.
///
/// SPEC: P2-PAGE-006, P2-PAGE-007.
pub(crate) fn write_subset_pdf(
    source: &PdfDocument<'_>,
    indices: &[i32],
    dest: &Path,
) -> Result<SaveOutcome, CommandError> {
    // Build the output document under the PDFium lock, then release it —
    // `save_document` re-acquires the lock (it is not reentrant).
    let out = {
        let _guard = pdfium_lock()?;
        let pdfium = pdfium()?;
        let mut out = pdfium.create_new_pdf().map_err(CommandError::from)?;
        out.pages_mut()
            .copy_pages_from_document(source, &range_string(indices), 0)
            .map_err(CommandError::from)?;
        out
    };

    // Reuse the explicit-save write path: atomic temp+rename + round-trip
    // verification. No backup — `dest` is a brand-new file.
    save_document(&out, dest, false)
}

/// Build a new PDF containing `pages` (0-based indices) of `source` and
/// write it to `dest`. Validates the indices against the live page count,
/// then delegates to [`write_subset_pdf`].
///
/// SPEC: P2-PAGE-006.
pub fn extract_pages(
    source: &PdfDocument<'_>,
    pages: Vec<i32>,
    dest: &Path,
) -> Result<SaveOutcome, CommandError> {
    let indices = {
        let _guard = pdfium_lock()?;
        validate(pages, source.pages().len())?
    };
    write_subset_pdf(source, &indices, dest)
}
