//! Merge multiple PDFs into one new file (P2.C4).
//!
//! SPEC: P2-PAGE-008 — concatenate N PDFs preserving **annotations,
//! bookmarks, and form fields**, suffixing colliding form-field names
//! (`_2`, `_3`, …). The merge is done in the lopdf COS layer
//! ([`crate::pdf::cos::merge_documents`]): each source's object graph is
//! renumbered and combined into one document, so nothing is lost (unlike
//! `FPDF_ImportPages`, which drops the `/Outlines` and `/AcroForm`). The
//! merged bytes are then loaded into `PDFium` and written through the verified
//! save path. Read-only on every input.

use std::path::{Path, PathBuf};

use crate::error::CommandError;
use crate::pdf::cos;
use crate::pdf::document::{pdfium, pdfium_lock, save_document, SaveOutcome};

/// Merge `sources` (≥ 2 files, in order) into a new PDF at `dest`, preserving
/// pages, annotations, bookmarks, and form fields (colliding field names are
/// suffixed). SPEC: P2-PAGE-008.
pub fn merge_documents(sources: &[PathBuf], dest: &Path) -> Result<SaveOutcome, CommandError> {
    if sources.len() < 2 {
        return Err(CommandError::InvalidInput(
            "merge needs at least two files".into(),
        ));
    }

    // Read every source's bytes up front. A failure here (missing / unreadable)
    // aborts before anything is written — never a partial merge.
    let mut source_bytes = Vec::with_capacity(sources.len());
    for path in sources {
        let bytes = std::fs::read(path).map_err(|e| {
            CommandError::InvalidInput(format!("cannot read {}: {e}", path.display()))
        })?;
        source_bytes.push(bytes);
    }

    // All-lopdf merge (pure Rust, no `PDFium` lock): combines the object graphs,
    // preserving outlines + form fields and renaming colliding /T names.
    let merged = cos::merge_documents(&source_bytes)?;

    // Load the merged bytes into `PDFium`, then write via the verified explicit-
    // save path (atomic temp+rename + round-trip reopen). Building `out` under
    // the lock; `save_document` re-acquires it (the lock is not reentrant).
    let out = {
        let _guard = pdfium_lock()?;
        pdfium()?
            .load_pdf_from_byte_vec(merged, None)
            .map_err(CommandError::from)?
    };

    save_document(&out, dest, false, None)
}
