//! Restore a document to an exact byte snapshot — a generic undo primitive.
//!
//! Some edits change so much at once (page tree + `/AcroForm` + …) that the
//! cheapest *correct* inverse is "put the document back to exactly these
//! bytes." Insert-from-PDF (P2.D1) uses this: undo restores the pre-insert
//! snapshot, redo restores the post-insert one. Like reorder, it replaces the
//! actor's live document via `load_pdf_from_byte_vec`; the old document is
//! dropped under the `PDFIUM_LOCK`.

use pdfium_render::prelude::PdfDocument;

use crate::error::CommandError;
use crate::pdf::document::{pdfium, pdfium_lock};
use crate::pdf::undo::Edit;

/// Replace the document with exactly `bytes` when applied. Its inverse is a
/// `RestoreDocEdit` holding the document's bytes from *before* the swap, so
/// undo and redo simply trade snapshots.
pub struct RestoreDocEdit {
    pub bytes: Vec<u8>,
}

impl<'a> Edit<PdfDocument<'a>> for RestoreDocEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        let RestoreDocEdit { bytes } = *self;

        // Snapshot the current state — it becomes the inverse (for redo).
        let current = {
            let _guard = pdfium_lock()?;
            doc.save_to_bytes().map_err(CommandError::from)?
        };

        // Replace the live document with our bytes. The old document is dropped
        // by this assignment, under the lock (its `FPDF_CloseDocument` must be
        // serialized like every FFI call); the new one owns its buffer.
        {
            let _guard = pdfium_lock()?;
            *doc = pdfium()?
                .load_pdf_from_byte_vec(bytes, None)
                .map_err(CommandError::from)?;
        }

        Ok(Box::new(RestoreDocEdit { bytes: current }))
    }

    fn label(&self) -> &'static str {
        "restore-document"
    }

    /// A full-document byte snapshot — the dominant undo-memory cost, and
    /// the reason the history needs a byte budget. `FABLE_REVIEW` §3.6.
    fn heap_bytes(&self) -> usize {
        self.bytes.len()
    }
}
