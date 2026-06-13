//! Resize a page — scale content to fit a new size — as an undoable edit (P2.B5).
//!
//! SPEC: P2-PAGE-010 — resize one or more pages to a standard or custom size,
//! scaling the page content to fit (with an optional preserve-aspect-ratio
//! mode), not merely relabeling the box.
//!
//! The scale is done at the COS level via the lopdf byte-handoff (the same
//! pattern as reorder/merge), **not** through `PDFium`: serialize the live
//! document → [`crate::pdf::cos::resize_pages`] wraps each page's content
//! stream with a `q <matrix> cm … Q` and sets the new `/MediaBox` → reload the
//! result, replacing the actor's document. `PDFium`'s page-content transform
//! (`FPDFPage_TransFormWithClip`) forces a `reload_in_place` that SIGSEGVs at
//! teardown (pdfium-render #93), so we keep content transforms out of `PDFium`.
//!
//! The inverse is a [`RestoreDocEdit`] holding the pre-resize bytes — the same
//! lossless snapshot-undo insert-from (P2.D1) uses; a scale is awkward to
//! invert exactly, and a snapshot is simplest and correct.

use pdfium_render::prelude::PdfDocument;

use crate::error::CommandError;
use crate::pdf::cos::resize_pages as cos_resize_pages;
use crate::pdf::document::{pdfium, pdfium_lock};
use crate::pdf::restore::RestoreDocEdit;
use crate::pdf::undo::Edit;

/// Resize `pages` (0-based) to `width` × `height` points. When
/// `preserve_aspect` is set, content is scaled uniformly by the smaller ratio
/// and centred in the new box; otherwise it is stretched to fill.
pub struct ResizeEdit {
    pub pages: Vec<i32>,
    pub width: f32,
    pub height: f32,
    pub preserve_aspect: bool,
}

impl<'a> Edit<PdfDocument<'a>> for ResizeEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        if self.width <= 0.0 || self.height <= 0.0 {
            return Err(CommandError::InvalidInput(format!(
                "resize dimensions must be positive, got {}×{}",
                self.width, self.height
            )));
        }
        if self.pages.is_empty() {
            return Err(CommandError::InvalidInput("no pages specified".into()));
        }

        // 0-based page indices as usize (reject negatives up front; the upper
        // bound is checked in cos::resize_pages against the real page tree).
        let mut pages = Vec::with_capacity(self.pages.len());
        for &p in &self.pages {
            let u = usize::try_from(p)
                .map_err(|_| CommandError::InvalidInput(format!("negative page index: {p}")))?;
            pages.push(u);
        }

        // 1. Serialize the live document (one locked FFI span). This is also the
        //    lossless inverse snapshot.
        let pre_bytes = {
            let _guard = pdfium_lock()?;
            doc.save_to_bytes().map_err(CommandError::from)?
        };

        // 2. Scale content + set the MediaBox via lopdf (pure Rust, no lock).
        //    Validates page indices, so a bad request fails here without
        //    touching the live document.
        let new_bytes = cos_resize_pages(&pre_bytes, &pages, self.width, self.height, self.preserve_aspect)?;

        // 3. Reload the resized bytes, replacing the actor's document (the old
        //    one is dropped under the lock). The reload is itself the round-trip
        //    verification — a malformed result would fail to open here.
        {
            let _guard = pdfium_lock()?;
            *doc = pdfium()?
                .load_pdf_from_byte_vec(new_bytes, None)
                .map_err(CommandError::from)?;
        }

        Ok(Box::new(RestoreDocEdit { bytes: pre_bytes }))
    }

    fn label(&self) -> &'static str {
        "resize"
    }
}
