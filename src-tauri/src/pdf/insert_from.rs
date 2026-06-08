//! Insert pages from another PDF as an undoable edit (P2.D1).
//!
//! SPEC: P2-PAGE-005 — copy selected pages of a source file into the open
//! document at a target index, preserving content, annotations, dimensions
//! (`MediaBox`), and **form fields**. `FPDF_ImportPages` copies the pages,
//! their content, dimensions, and (widget) annotations, but doesn't link the
//! form fields into the document `/AcroForm`; a lopdf pass
//! ([`crate::pdf::cos::register_inserted_form_fields`]) re-attaches the
//! inserted pages' terminal form fields (suffixing colliding `/T` names).
//!
//! Because the form-field re-attach is a dict-level change `PDFium` can't undo
//! piecemeal, the inverse is a [`RestoreDocEdit`] holding the *pre-insert*
//! document bytes: undo restores that snapshot, redo restores the post-insert
//! one. (Snapshots, not a `DeleteEdit`, so the `/AcroForm` change round-trips.)

use std::path::PathBuf;

use pdfium_render::prelude::PdfDocument;

use crate::error::CommandError;
use crate::pdf::cos::register_inserted_form_fields;
use crate::pdf::delete_page::{range_string, validate};
use crate::pdf::document::{open_pdf, pdfium, pdfium_lock};
use crate::pdf::restore::RestoreDocEdit;
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

        // Snapshot the pre-insert document — it becomes the undo target.
        let pre_bytes = {
            let _guard = pdfium_lock()?;
            doc.save_to_bytes().map_err(CommandError::from)?
        };

        // Open the source first: `open_pdf` takes the lock internally, so we
        // must not be holding it here (the lock is not reentrant).
        let (source, _meta) = open_pdf(&source_path, None).map_err(|e| {
            CommandError::InvalidInput(format!("cannot open {}: {e}", source_path.display()))
        })?;

        // Copy the pages under the lock, then close the source under the lock
        // too — `Drop` would otherwise call `FPDF_CloseDocument` unlocked,
        // racing other PDFium threads. Done on both success and error paths.
        let result = copy_under_lock(doc, &source, pages, index);
        {
            let _close = pdfium_lock().ok();
            drop(source);
        }
        let inserted = result?;

        // Re-attach the inserted pages' form fields (PDFium copies the widgets
        // but not the /AcroForm linkage): serialize, run the lopdf pass, reload.
        let start = usize::try_from(index)
            .map_err(|_| CommandError::Internal("negative insert index".into()))?;
        let post_bytes = {
            let _guard = pdfium_lock()?;
            doc.save_to_bytes().map_err(CommandError::from)?
        };
        let fixed = register_inserted_form_fields(&post_bytes, start, inserted)?;
        {
            let _guard = pdfium_lock()?;
            *doc = pdfium()?
                .load_pdf_from_byte_vec(fixed, None)
                .map_err(CommandError::from)?;
        }

        // Inverse: restore the document to its pre-insert bytes (pages + form).
        Ok(Box::new(RestoreDocEdit { bytes: pre_bytes }))
    }

    fn label(&self) -> &'static str {
        "insert-from-pdf"
    }
}
