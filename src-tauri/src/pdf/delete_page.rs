//! Page deletion as an undoable edit (P2.B2).
//!
//! SPEC: P2-PAGE-003 — delete one or more pages. `PDFium` renumbers the
//! page tree; internal references that target *surviving* pages stay
//! correct automatically, because PDF destinations are indirect object
//! references (to the page dict), not page indices — deleting a page and
//! renumbering doesn't move the object a reference points to.
//!
//! Deletion is undoable: the removed pages are stashed in a holding
//! document (serialized to bytes) and re-imported on undo, preserving
//! content and order.
//!
//! Limitation: pdfium-render's outline/link/destination API is read-only,
//! so we cannot actively rewrite or remove references *to* a deleted page
//! (they become dangling). Surviving-target integrity is covered by
//! `tests/delete_page.rs`; active reference rewriting is deferred (BACKLOG
//! — would need dict-level access).

use pdfium_render::prelude::PdfDocument;

use crate::error::CommandError;
use crate::pdf::document::{pdfium, pdfium_lock};
use crate::pdf::undo::Edit;

/// Delete `pages` (0-based indices) from the document.
pub struct DeleteEdit {
    pub pages: Vec<i32>,
}

/// The inverse of a delete: re-import the removed pages — held as a
/// serialized single-document `bytes` — back at their original `indices`.
struct RestorePagesEdit {
    bytes: Vec<u8>,
    indices: Vec<i32>,
}

/// 0-based indices → a 1-based `PDFium` page-range string, e.g.
/// `[1, 3, 4]` → `"2,4,5"`. (`FPDF_ImportPages` ranges are 1-based.)
/// Shared with `pdf::extract` (P2.C2).
pub(crate) fn range_string(indices: &[i32]) -> String {
    indices
        .iter()
        .map(|i| (i + 1).to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// Normalize a caller-supplied index list: sort ascending, de-dup, and
/// validate every entry against `count`. Returns the clean list or a
/// typed error *before* any mutation (so a bad index can't leave the
/// document half-edited). Shared with `pdf::extract` (P2.C2).
pub(crate) fn validate(mut indices: Vec<i32>, count: i32) -> Result<Vec<i32>, CommandError> {
    indices.sort_unstable();
    indices.dedup();
    if indices.is_empty() {
        return Err(CommandError::InvalidInput("no pages specified".into()));
    }
    for &idx in &indices {
        if idx < 0 || idx >= count {
            return Err(CommandError::InvalidInput(format!(
                "page index out of range: {idx} (document has {count} pages)"
            )));
        }
    }
    Ok(indices)
}

impl<'a> Edit<PdfDocument<'a>> for DeleteEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        let DeleteEdit { pages } = *self;
        let _guard = pdfium_lock()?;

        let indices = validate(pages, doc.pages().len())?;

        // Stash the doomed pages in a holding document (content preserved
        // for undo), serialized to bytes.
        let pdfium = pdfium()?;
        let mut holding = pdfium.create_new_pdf().map_err(CommandError::from)?;
        holding
            .pages_mut()
            .copy_pages_from_document(doc, &range_string(&indices), 0)
            .map_err(CommandError::from)?;
        let bytes = holding.save_to_bytes().map_err(CommandError::from)?;
        drop(holding);

        // Delete descending so earlier removals don't shift later indices.
        for &idx in indices.iter().rev() {
            doc.pages()
                .get(idx)
                .map_err(CommandError::from)?
                .delete()
                .map_err(CommandError::from)?;
        }

        Ok(Box::new(RestorePagesEdit { bytes, indices }))
    }

    fn label(&self) -> &'static str {
        "delete"
    }
}

impl<'a> Edit<PdfDocument<'a>> for RestorePagesEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        let RestorePagesEdit { bytes, indices } = *self;
        let _guard = pdfium_lock()?;

        let pdfium = pdfium()?;
        let holding = pdfium
            .load_pdf_from_byte_vec(bytes, None)
            .map_err(CommandError::from)?;

        // Re-insert each held page at its original index. `indices` is
        // ascending, so inserting in order is correct: each insert shifts
        // only the still-pending (larger) targets, which haven't been
        // placed yet. Held page `n` (1-based "n+1") maps to indices[n].
        for (held_pos, &target) in indices.iter().enumerate() {
            doc.pages_mut()
                .copy_pages_from_document(&holding, &(held_pos + 1).to_string(), target)
                .map_err(CommandError::from)?;
        }
        drop(holding);

        Ok(Box::new(DeleteEdit { pages: indices }))
    }

    fn label(&self) -> &'static str {
        "restore-pages"
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn range_string_is_one_based_and_comma_joined() {
        assert_eq!(range_string(&[0]), "1");
        assert_eq!(range_string(&[1, 3, 4]), "2,4,5");
    }

    #[test]
    fn validate_sorts_dedups_and_range_checks() {
        assert_eq!(validate(vec![3, 1, 1, 0], 5).unwrap(), vec![0, 1, 3]);
        assert!(validate(vec![], 5).is_err());
        assert!(validate(vec![5], 5).is_err()); // out of range (0..5)
        assert!(validate(vec![-1], 5).is_err());
    }
}
