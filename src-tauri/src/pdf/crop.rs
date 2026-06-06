//! Crop a page via its `/CropBox` as an undoable edit (P2.B4).
//!
//! SPEC: P2-PAGE-009 — adjust the `/CropBox` without touching the content
//! streams (the box is just the visible window); reversible by setting it
//! back to the `/MediaBox`. Implemented as an [`Edit`] so it integrates
//! with undo (A3): `apply` captures the previous box and returns an inverse
//! that restores it (so "reset crop" is itself undoable).

use pdfium_render::prelude::{PdfDocument, PdfRect};

use crate::error::CommandError;
use crate::pdf::document::pdfium_lock;
use crate::pdf::undo::Edit;

/// Crop `page` to `rect` = (left, bottom, right, top) in points. `None`
/// resets the `/CropBox` to the `/MediaBox`.
pub struct CropEdit {
    pub page: i32,
    pub rect: Option<(f32, f32, f32, f32)>,
}

impl<'a> Edit<PdfDocument<'a>> for CropEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        let _guard = pdfium_lock()?;

        let count = doc.pages().len();
        if self.page < 0 || self.page >= count {
            return Err(CommandError::InvalidInput(format!(
                "page index out of range: {} (document has {count} pages)",
                self.page
            )));
        }
        if let Some((l, b, r, t)) = self.rect {
            if l >= r || b >= t {
                return Err(CommandError::InvalidInput(format!(
                    "invalid crop rect: left {l} ≥ right {r}, or bottom {b} ≥ top {t}"
                )));
            }
        }

        let pages = doc.pages();
        let mut page = pages.get(self.page).map_err(CommandError::from)?;

        // Capture the current CropBox so the inverse can restore it exactly.
        // PDFium errors on `crop()` when no explicit `/CropBox` is set, in
        // which case the effective box is the `/MediaBox`.
        let old = page
            .boundaries()
            .crop()
            .or_else(|_| page.boundaries().media())
            .map_err(CommandError::from)?
            .bounds;
        let old_rect = (
            old.left().value,
            old.bottom().value,
            old.right().value,
            old.top().value,
        );

        // Target: the requested rect, or the MediaBox on reset.
        let target = match self.rect {
            Some((l, b, r, t)) => PdfRect::new_from_values(b, l, t, r),
            None => page.boundaries().media().map_err(CommandError::from)?.bounds,
        };
        page.boundaries_mut()
            .set_crop(target)
            .map_err(CommandError::from)?;

        Ok(Box::new(CropEdit {
            page: self.page,
            rect: Some(old_rect),
        }))
    }

    fn label(&self) -> &'static str {
        "crop"
    }
}
