//! Insert a blank page as an undoable edit (P2.B3).
//!
//! SPEC: P2-PAGE-004 — insert a blank page, inheriting the adjacent page's
//! size and orientation (both carried by its width/height) unless an
//! explicit size is given.
//!
//! The inverse of an insert is a delete, so this reuses `DeleteEdit`
//! (P2.B2): undo removes the blank, redo re-inserts it. Inserting a page
//! rewrites no references (it adds an object nothing points to), so unlike
//! a page move there is nothing to fix up.

use pdfium_render::prelude::{PdfDocument, PdfPagePaperSize, PdfPoints};

use crate::error::CommandError;
use crate::pdf::delete_page::DeleteEdit;
use crate::pdf::document::pdfium_lock;
use crate::pdf::undo::Edit;

/// US Letter (points), used when an empty document has no neighbour to
/// inherit from.
const DEFAULT_SIZE_PT: (f32, f32) = (612.0, 792.0);

/// Insert one blank page at `index` (0-based). `size` (width, height in
/// points) overrides the inherited dimensions when present.
pub struct InsertBlankEdit {
    pub index: i32,
    pub size: Option<(f32, f32)>,
}

/// The (width, height) in points to give a blank page inserted at `index`:
/// the explicit override, else the adjacent page's size, else US Letter.
fn resolve_size(
    doc: &PdfDocument<'_>,
    index: i32,
    count: i32,
    override_size: Option<(f32, f32)>,
) -> Result<(f32, f32), CommandError> {
    if let Some(wh) = override_size {
        return Ok(wh);
    }
    if count == 0 {
        return Ok(DEFAULT_SIZE_PT);
    }
    // Inherit from the page before the insertion point, or — when inserting
    // at the very start — the page that will follow it.
    let neighbour = if index > 0 { index - 1 } else { 0 };
    let page = doc.pages().get(neighbour).map_err(CommandError::from)?;
    Ok((page.width().value, page.height().value))
}

impl<'a> Edit<PdfDocument<'a>> for InsertBlankEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        let _guard = pdfium_lock()?;
        let count = doc.pages().len();

        // Append-at-end (index == count) is valid; beyond that is not.
        if self.index < 0 || self.index > count {
            return Err(CommandError::InvalidInput(format!(
                "insert index out of range: {} (0..={count})",
                self.index
            )));
        }

        let (w, h) = resolve_size(doc, self.index, count, self.size)?;
        let size = PdfPagePaperSize::Custom(PdfPoints::new(w), PdfPoints::new(h));
        doc.pages_mut()
            .create_page_at_index(size, self.index)
            .map_err(CommandError::from)?;

        // The inverse is a delete of the page we just inserted.
        Ok(Box::new(DeleteEdit {
            pages: vec![self.index],
        }))
    }

    fn label(&self) -> &'static str {
        "insert-blank"
    }
}
