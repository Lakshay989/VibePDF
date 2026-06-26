//! In-place text-run editing (P4.A3) — the reflow half of redact-and-reflow.
//!
//! SPEC: infrastructure for P4-EDIT-001 (edit existing text). Given a text run
//! located by the *same* index [`crate::pdf::text_extract::extract_text_runs`]
//! (A1) produces, it rewrites that run's text in the page content stream and
//! returns new document bytes.
//!
//! ## Why `PDFium`, and why bytes → bytes
//! This is the project's first write that uses `PDFium`'s own object-mutation API
//! (`FPDFText_SetText`) rather than the lopdf COS path — re-emitting glyphs is
//! precisely what `PDFium` does well. But we never mutate the actor's long-lived
//! document in place: `PDFium` content mutation can SIGSEGV at teardown (see
//! `docs/04` and [`crate::pdf::cos::resize_pages`]). So, like every COS edit, we
//! mutate a **throwaway** document loaded from the input bytes, serialize it, and
//! let [`ReplaceTextRunEdit`] swap the live document to the result — with the
//! pre-edit byte snapshot as the inverse ([`RestoreDocEdit`]).
//!
//! ## Scope: edit only (the *redact* half is deferred)
//! "Two-phase redact-and-reflow" needs object **removal** for its redact half
//! (delete a run, true redaction, and recreating a run in a substitute font). Our
//! bundled `PDFium`'s `FPDFPage_RemoveObject` SIGSEGVs, so A3 ships only the
//! **edit** half: [`replace_text_run`] via `set_text`, which preserves the run's
//! font, size, colour and matrix. Editing a run whose font isn't embedded still
//! succeeds, but the (non-embedded) font reference is kept — A2's once-per-document
//! warning already flags that such text may render in a substitute. Baking the
//! substitute face into the file, deletion, and redaction all wait on a removal
//! path that doesn't crash (lopdf content-stream surgery; see `BACKLOG.md`).
//!
//! Neighbour-run reflow and in-bbox line wrapping are likewise **not** A3's job —
//! they need the whole-line layout model only the B1 editor has.

use pdfium_render::prelude::*;

use crate::error::CommandError;
use crate::pdf::document::{pdfium, pdfium_lock};
use crate::pdf::restore::RestoreDocEdit;
use crate::pdf::undo::Edit;

/// SPEC: P4-EDIT-001 — replace run `run_index` on `page` (0-based, A1 ordering)
/// with `new_text`, preserving its font/size/colour/position. Returns new
/// document bytes; never mutates the input.
pub fn replace_text_run(
    bytes: &[u8],
    page: usize,
    run_index: usize,
    new_text: &str,
) -> Result<Vec<u8>, CommandError> {
    let page_index = i32::try_from(page)
        .map_err(|_| CommandError::InvalidInput(format!("bad page index: {page}")))?;

    let _guard = pdfium_lock()?;
    let doc = pdfium()?
        .load_pdf_from_byte_vec(bytes.to_vec(), None)
        .map_err(CommandError::from)?;

    // Single page borrow: locate + mutate + regenerate. Staging under **Manual**
    // regeneration and committing once is load-bearing — `set_text` mutates the
    // object handle but does not itself flag the page, so without an explicit
    // `regenerate_content` the change is lost on save.
    {
        let mut pdf_page = doc.pages().get(page_index).map_err(CommandError::from)?;
        pdf_page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
        let obj_index = nth_text_object_index(&pdf_page, run_index)?;

        let mut object = pdf_page.objects().get(obj_index).map_err(CommandError::from)?;
        let PdfPageObject::Text(text_object) = &mut object else {
            return Err(CommandError::Internal("located run is not a text object".into()));
        };
        text_object.set_text(new_text).map_err(CommandError::from)?;

        pdf_page.regenerate_content().map_err(CommandError::from)?;
    }

    let out = doc.save_to_bytes().map_err(CommandError::from)?;
    // Drop the throwaway document under the lock (its FPDF_CloseDocument is FFI).
    drop(doc);
    Ok(out)
}

/// Find the container index of the `run_index`-th **text** object on the page,
/// counting in the same order A1's `extract_text_runs` iterates — so a frontend
/// hit-test index maps straight through.
fn nth_text_object_index(page: &PdfPage, run_index: usize) -> Result<usize, CommandError> {
    let mut text_seen = 0usize;
    for (container_index, object) in page.objects().iter().enumerate() {
        if matches!(object, PdfPageObject::Text(_)) {
            if text_seen == run_index {
                return Ok(container_index);
            }
            text_seen += 1;
        }
    }
    Err(CommandError::InvalidInput(format!(
        "run index {run_index} out of range ({text_seen} text runs on page)"
    )))
}

/// SPEC: P4-EDIT-001 — replace a run as one undoable edit. Mutates a throwaway
/// doc, swaps the actor's live document to the result, records the pre-edit
/// snapshot as the inverse. Mirrors [`crate::pdf::annotation`]'s `cos_edit`.
pub struct ReplaceTextRunEdit {
    pub page: usize,
    pub run_index: usize,
    pub new_text: String,
}

impl<'a> Edit<PdfDocument<'a>> for ReplaceTextRunEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        // Snapshot the live document (the lossless inverse), run the bytes → bytes
        // reflow, then reload/replace the document. Same shape as the COS
        // `cos_edit`, but the transform is a `PDFium` mutation rather than lopdf.
        let pre_bytes = {
            let _guard = pdfium_lock()?;
            doc.save_to_bytes().map_err(CommandError::from)?
        };
        let new_bytes = replace_text_run(&pre_bytes, self.page, self.run_index, &self.new_text)?;
        {
            let _guard = pdfium_lock()?;
            *doc = pdfium()?
                .load_pdf_from_byte_vec(new_bytes, None)
                .map_err(CommandError::from)?;
        }
        Ok(Box::new(RestoreDocEdit { bytes: pre_bytes }))
    }

    fn label(&self) -> &'static str {
        "replace-text-run"
    }
}
