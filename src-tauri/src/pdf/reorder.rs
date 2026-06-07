//! Reorder pages as an undoable edit (P2.C1).
//!
//! SPEC: P2-PAGE-002 — drag a thumbnail to a new position; renumber pages and
//! keep internal references valid. Reordering the page tree is dict-level work
//! `PDFium`'s API can't do, so it goes through the lopdf COS layer
//! ([`crate::pdf::cos::reorder_pages`]) on a byte round-trip: serialize the live
//! document → reorder `/Kids` → reload the result, **replacing** the actor's
//! document. Object-ref links, bookmarks, and named destinations reference page
//! objects (not positions), so they track the move for free.
//!
//! The inverse of a reorder is the inverse permutation, so undo/redo store only
//! a `Vec<usize>` — no full-document byte snapshots.

use pdfium_render::prelude::PdfDocument;

use crate::error::CommandError;
use crate::pdf::cos::reorder_pages;
use crate::pdf::document::{pdfium, pdfium_lock};
use crate::pdf::undo::Edit;

/// Reorder pages: `order[new_pos] = old_index` (0-based), a permutation of
/// `0..page_count`.
pub struct ReorderEdit {
    pub order: Vec<usize>,
}

/// Invert a permutation `p` (`p[new] = old`) into `q` (`q[old] = new`).
fn invert(order: &[usize]) -> Vec<usize> {
    let mut inv = vec![0usize; order.len()];
    for (new_pos, &old) in order.iter().enumerate() {
        inv[old] = new_pos;
    }
    inv
}

impl<'a> Edit<PdfDocument<'a>> for ReorderEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        let ReorderEdit { order } = *self;

        // 1. Serialize the live document (one locked FFI span).
        let bytes = {
            let _guard = pdfium_lock()?;
            doc.save_to_bytes().map_err(CommandError::from)?
        };

        // 2. Reorder the page tree via lopdf (pure Rust, no lock). This also
        //    validates the permutation and the flat-tree precondition, so a bad
        //    request fails here without touching the live document.
        let new_bytes = reorder_pages(&bytes, &order)?;

        // 3. Reload the reordered bytes, replacing the actor's document. The old
        //    document is dropped by this assignment, under the lock — its
        //    `FPDF_CloseDocument` must be serialized like every other FFI call.
        //    `load_pdf_from_byte_vec` takes ownership of the buffer, so the new
        //    document is `'static` (it owns what it reads from).
        {
            let _guard = pdfium_lock()?;
            let pdfium = pdfium()?;
            *doc = pdfium
                .load_pdf_from_byte_vec(new_bytes, None)
                .map_err(CommandError::from)?;
        }

        // 4. The inverse reorder is the inverse permutation.
        Ok(Box::new(ReorderEdit { order: invert(&order) }))
    }

    fn label(&self) -> &'static str {
        "reorder"
    }
}
