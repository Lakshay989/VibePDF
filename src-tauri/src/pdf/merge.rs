//! Merge multiple PDFs into one new file (P2.C4).
//!
//! SPEC: P2-PAGE-008 (PARTIAL) — concatenates all pages of each source, in
//! order, into a new document, preserving page-level annotations (carried by
//! `FPDF_ImportPages`). Bookmarks, interactive form fields (`AcroForm`), and
//! form-field collision renaming are NOT handled: `FPDF_ImportPages` copies
//! neither the document `/Outlines` tree nor the `/AcroForm`, and renaming
//! needs dict-level access. Those three criteria are deferred to a follow-up
//! that adds `lopdf` (see `BACKLOG.md` / `docs/03_TECH_STACK.md`). Read-only
//! on every input — nothing is mutated, the open document is untouched.

use std::path::{Path, PathBuf};

use crate::error::CommandError;
use crate::pdf::document::{open_pdf, pdfium, pdfium_lock, save_document, SaveOutcome};

/// Merge `sources` (≥ 2 files, in order) into a new PDF at `dest`. Every page
/// of each source is appended in turn. Page-level annotations are preserved;
/// bookmarks and form fields are not (see module docs).
///
/// SPEC: P2-PAGE-008 (partial).
pub fn merge_documents(sources: &[PathBuf], dest: &Path) -> Result<SaveOutcome, CommandError> {
    if sources.len() < 2 {
        return Err(CommandError::InvalidInput(
            "merge needs at least two files".into(),
        ));
    }

    // Open every source up front. A failure here (missing / corrupt /
    // encrypted) aborts before anything is written — never a partial merge.
    let mut docs = Vec::with_capacity(sources.len());
    for path in sources {
        let (doc, _meta) = open_pdf(path, None).map_err(|e| {
            CommandError::InvalidInput(format!("cannot open {}: {e}", path.display()))
        })?;
        docs.push(doc);
    }

    // Build the merged document under a single PDFium lock span, then release
    // it before `save_document` re-locks (the lock is not reentrant).
    let out = {
        let _guard = pdfium_lock()?;
        let pdfium = pdfium()?;
        let mut out = pdfium.create_new_pdf().map_err(CommandError::from)?;
        for doc in &docs {
            let count = doc.pages().len();
            if count < 1 {
                return Err(CommandError::InvalidInput(
                    "a source document has no pages".into(),
                ));
            }
            // Append all of this source's pages at the current end index.
            let at = out.pages().len();
            let range = format!("1-{count}");
            out.pages_mut()
                .copy_pages_from_document(doc, &range, at)
                .map_err(CommandError::from)?;
        }
        out
    };

    // Reuse the verified explicit-save path: atomic temp+rename + round-trip
    // reopen. No backup — `dest` is a brand-new file.
    save_document(&out, dest, false)
}
