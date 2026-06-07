//! Insert pages from another PDF as an undoable edit (P2.D1).
//!
//! SPEC: P2-PAGE-005 (PARTIAL) — copy selected pages of a source file into the
//! open document at a target index. `FPDF_ImportPages` carries each page's
//! content, page-level annotations, and dimensions (its `MediaBox`). Interactive
//! form fields (`/AcroForm`) are NOT carried — that's dict-level work deferred
//! to the lopdf follow-up (see `BACKLOG.md`).
//!
//! The inverse of an insert is a delete, so this reuses `DeleteEdit` (P2.B2)
//! over the inserted block: undo removes the imported pages, and `DeleteEdit`'s
//! holding-doc keeps their bytes so redo restores them even if the source file
//! is later moved or deleted.

use std::path::PathBuf;

use pdfium_render::prelude::PdfDocument;

use crate::error::CommandError;
use crate::pdf::delete_page::{range_string, validate, DeleteEdit};
use crate::pdf::document::{open_pdf, pdfium_lock};
use crate::pdf::undo::Edit;

/// Insert `pages` (0-based indices into `source_path`) into the open document
/// at `index` (0-based; `index == page_count` appends).
pub struct InsertFromEdit {
    pub source_path: PathBuf,
    pub pages: Vec<i32>,
    pub index: i32,
}

/// Copy the selected source pages into `doc` at `index`, under the lock.
/// Returns how many pages were inserted. Split out so `apply` can keep this
/// FFI span (and its guard) separate from opening/closing the source.
fn copy_under_lock(
    doc: &mut PdfDocument<'_>,
    source: &PdfDocument<'_>,
    pages: Vec<i32>,
    index: i32,
) -> Result<usize, CommandError> {
    let _guard = pdfium_lock()?;
    let dest_count = doc.pages().len();
    // Append-at-end (index == count) is valid; beyond that is not.
    if index < 0 || index > dest_count {
        return Err(CommandError::InvalidInput(format!(
            "insert index out of range: {index} (0..={dest_count})"
        )));
    }
    // `validate` sorts, de-dups, and bounds-checks against the source count.
    let indices = validate(pages, source.pages().len())?;
    doc.pages_mut()
        .copy_pages_from_document(source, &range_string(&indices), index)
        .map_err(CommandError::from)?;
    Ok(indices.len())
}

impl<'a> Edit<PdfDocument<'a>> for InsertFromEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        let InsertFromEdit { source_path, pages, index } = *self;

        // Open the source first: `open_pdf` takes the lock internally, so we
        // must not be holding it here (the lock is not reentrant).
        let (source, _meta) = open_pdf(&source_path, None).map_err(|e| {
            CommandError::InvalidInput(format!("cannot open {}: {e}", source_path.display()))
        })?;

        // Copy under the lock, then close the source under the lock too —
        // `Drop` would otherwise call `FPDF_CloseDocument` unlocked, racing
        // other PDFium threads. Done on both success and error paths.
        let result = copy_under_lock(doc, &source, pages, index);
        {
            let _close = pdfium_lock().ok();
            drop(source);
        }
        let inserted = result?;

        let n = i32::try_from(inserted)
            .map_err(|_| CommandError::Internal("inserted page count overflow".into()))?;
        // The inverse is a delete of the contiguous block we just inserted.
        let block: Vec<i32> = (index..index + n).collect();
        Ok(Box::new(DeleteEdit { pages: block }))
    }

    fn label(&self) -> &'static str {
        "insert-from-pdf"
    }
}
