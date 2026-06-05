//! Page rotation as an undoable edit (P2.B1).
//!
//! SPEC: P2-PAGE-001 — rotation is persisted as `PDFium` `/Rotate` on each
//! page dict (not a viewer-only transform), so it survives save/reopen and
//! shows in any reader. Implemented as an [`Edit`] so it plugs straight
//! into the undo/redo stack (P2.A3): `apply` rotates the pages and returns
//! the inverse rotation.

use pdfium_render::prelude::{PdfDocument, PdfPageRenderRotation};

use crate::error::CommandError;
use crate::pdf::document::pdfium_lock;
use crate::pdf::undo::Edit;

/// Rotate one or more pages by a multiple of 90°.
///
/// `quarter_turns` is the clockwise *delta* (1 = 90°, 2 = 180°, 3 or -1 =
/// 270°/-90°) applied on top of each page's current `/Rotate`. Rotation is
/// additive mod 4, so the inverse is simply `-quarter_turns` — there's no
/// need to remember each page's prior angle.
pub struct RotateEdit {
    pub pages: Vec<i32>,
    pub quarter_turns: i32,
}

fn rotation_to_quarter(r: PdfPageRenderRotation) -> i32 {
    match r {
        PdfPageRenderRotation::None => 0,
        PdfPageRenderRotation::Degrees90 => 1,
        PdfPageRenderRotation::Degrees180 => 2,
        PdfPageRenderRotation::Degrees270 => 3,
    }
}

fn quarter_to_rotation(q: i32) -> PdfPageRenderRotation {
    match q.rem_euclid(4) {
        1 => PdfPageRenderRotation::Degrees90,
        2 => PdfPageRenderRotation::Degrees180,
        3 => PdfPageRenderRotation::Degrees270,
        _ => PdfPageRenderRotation::None,
    }
}

impl<'a> Edit<PdfDocument<'a>> for RotateEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        // Page lookup + mutation touch PDFium global state; serialize the
        // whole edit through the shared lock (see `document::PDFIUM_LOCK`).
        let _guard = pdfium_lock()?;

        let pages = doc.pages();
        let count = pages.len();

        // Validate every index *before* mutating any page, so a bad index
        // can't leave the document half-rotated with no undo entry. The
        // only realistic failure mode is out-of-range, so this makes the
        // edit effectively atomic.
        for &idx in &self.pages {
            if idx < 0 || idx >= count {
                return Err(CommandError::InvalidInput(format!(
                    "page index out of range: {idx} (document has {count} pages)"
                )));
            }
        }

        for &idx in &self.pages {
            let mut page = pages.get(idx).map_err(CommandError::from)?;
            let current = page.rotation().map_err(CommandError::from)?;
            let next = quarter_to_rotation(rotation_to_quarter(current) + self.quarter_turns);
            page.set_rotation(next);
        }

        Ok(Box::new(RotateEdit {
            pages: self.pages,
            quarter_turns: -self.quarter_turns,
        }))
    }

    fn label(&self) -> &'static str {
        "rotate"
    }
}
