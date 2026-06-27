//! Text-run editing and deletion (P4.A3 / P4.B3) — redact-and-reflow.
//!
//! SPEC: P4-EDIT-001 (edit existing text), P4-EDIT-004 (delete text), and the
//! (a)+(c) core of P6-SEC-010 (true redaction). Given a text run located by the
//! *same* index [`crate::pdf::text_extract::extract_text_runs`] (A1) produces, it
//! either rewrites or removes that run and returns new document bytes.
//!
//! ## Edit — `PDFium` `set_text`, bytes → bytes
//! [`replace_text_run`] uses `PDFium`'s own object-mutation API (`FPDFText_SetText`)
//! — re-emitting glyphs is precisely what `PDFium` does well. We never mutate the
//! actor's long-lived document in place (`PDFium` content mutation can SIGSEGV at
//! teardown — see `docs/04` and [`crate::pdf::cos::resize_pages`]); instead we
//! mutate a **throwaway** document loaded from the input bytes, serialize, and let
//! [`ReplaceTextRunEdit`] swap the live document to the result, with the pre-edit
//! byte snapshot as the inverse ([`RestoreDocEdit`]).
//!
//! ## Delete — lopdf content-stream surgery (the redact half)
//! `PDFium`'s `FPDFPage_RemoveObject` SIGSEGVs in our bundled build, so [`delete_text_run`]
//! removes a run at the **lopdf COS level**: decode the page content into operators,
//! splice out the run's text-showing operator (`Tj`/`TJ`), re-encode. The hard part
//! is that A1's `run_index` counts `PDFium` *text objects* while lopdf sees *show
//! operators* — for normal pages these align, but to never silently delete the wrong
//! run we **verify by re-extraction**: the post-delete run sequence must equal the
//! pre-delete one with exactly that index removed, else we error and the input bytes
//! are untouched. That verification is also P6-SEC-010(c). `'`/`"` (which carry a
//! line advance) and text inside `XObject`s are rejected, not silently mishandled.
//!
//! Neighbour-run reflow and in-bbox line wrapping are **not** done here — they need
//! the whole-line layout model only the B1 editor has.

use lopdf::content::Content;
use lopdf::Document;
use pdfium_render::prelude::*;

use crate::error::CommandError;
use crate::pdf::document::{pdfium, pdfium_lock};
use crate::pdf::restore::RestoreDocEdit;
use crate::pdf::text_extract::extract_text_runs;
use crate::pdf::undo::Edit;

/// Map a lopdf error so this can be used directly as a `.map_err(lopdf_err)` adapter.
#[allow(clippy::needless_pass_by_value)]
fn lopdf_err(e: lopdf::Error) -> CommandError {
    CommandError::PdfError(format!("lopdf: {e}"))
}

/// The PDF text-showing operators. Each paints one run (≈ one `PDFium` text object).
fn is_show_operator(operator: &str) -> bool {
    matches!(operator, "Tj" | "TJ" | "'" | "\"")
}

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

/// SPEC: P4-EDIT-004 / P6-SEC-010 — remove run `run_index` on `page` (0-based, A1
/// ordering) from the page content stream entirely. Returns new document bytes;
/// never mutates the input. **Safe by verification:** the result is re-extracted
/// and must equal the original run sequence with exactly the target removed, or
/// this errors (no silent corruption — see the module docs).
pub fn delete_text_run(
    bytes: &[u8],
    page: usize,
    run_index: usize,
) -> Result<Vec<u8>, CommandError> {
    // 1. The before-state, via PDFium — the authority on run ordering.
    let before = extract_runs(bytes, page)?;
    if run_index >= before.len() {
        return Err(CommandError::InvalidInput(format!(
            "run index {run_index} out of range ({} runs on page)",
            before.len()
        )));
    }

    // 2. Splice out the run_index-th show operator at the lopdf level.
    let new_bytes = splice_out_show_operator(bytes, page, run_index)?;

    // 3. Verify: the remaining runs must be exactly the original sequence minus the
    //    target. A mismatch means PDFium order ≠ content-stream order (e.g. text in
    //    an XObject) — reject rather than risk deleting the wrong content.
    let after = extract_runs(&new_bytes, page)?;
    let expected: Vec<&str> = before
        .iter()
        .enumerate()
        .filter_map(|(i, r)| (i != run_index).then_some(r.text.as_str()))
        .collect();
    let actual: Vec<&str> = after.iter().map(|r| r.text.as_str()).collect();
    if actual != expected {
        return Err(CommandError::PdfError(
            "text deletion did not match the located run — its glyphs may live in an \
             XObject or a content-stream operator we don't rewrite"
                .to_owned(),
        ));
    }
    Ok(new_bytes)
}

/// Load `bytes` into a throwaway `PDFium` document and extract `page`'s runs.
/// Loading needs the lock; `extract_text_runs` takes it itself, so we don't hold
/// it across the call. The document is closed under the lock (FFI).
fn extract_runs(bytes: &[u8], page: usize) -> Result<Vec<crate::pdf::text_extract::TextRun>, CommandError> {
    let doc = {
        let _guard = pdfium_lock()?;
        pdfium()?
            .load_pdf_from_byte_vec(bytes.to_vec(), None)
            .map_err(CommandError::from)?
    };
    let runs = extract_text_runs(&doc, page)?;
    {
        let _guard = pdfium_lock()?;
        drop(doc);
    }
    Ok(runs)
}

/// Decode the page content, drop the `run_index`-th text-showing operator, re-encode.
/// Pure lopdf — no `PDFium`, no lock. Rejects `'`/`"` (they also advance the line).
fn splice_out_show_operator(
    bytes: &[u8],
    page: usize,
    run_index: usize,
) -> Result<Vec<u8>, CommandError> {
    let mut doc = Document::load_mem(bytes).map_err(lopdf_err)?;
    let page_no = u32::try_from(page)
        .ok()
        .map(|n| n + 1)
        .ok_or_else(|| CommandError::InvalidInput(format!("bad page index: {page}")))?;
    let page_id = *doc
        .get_pages()
        .get(&page_no)
        .ok_or_else(|| CommandError::InvalidInput(format!("page index out of range: {page}")))?;

    let mut operations = doc.get_and_decode_page_content(page_id).map_err(lopdf_err)?.operations;

    // Find the run_index-th show operator (counting Tj/TJ/'/" — each is one run).
    let mut seen = 0usize;
    let mut target = None;
    for (i, op) in operations.iter().enumerate() {
        if is_show_operator(&op.operator) {
            if seen == run_index {
                target = Some(i);
                break;
            }
            seen += 1;
        }
    }
    let idx = target.ok_or_else(|| {
        CommandError::InvalidInput(format!(
            "run index {run_index} out of range ({seen} show operators in page content)"
        ))
    })?;

    // `'` and `"` move to the next line before showing; removing them would shift the
    // following text. Out of scope — reject cleanly (these are rare).
    let operator = &operations[idx].operator;
    if operator == "'" || operator == "\"" {
        return Err(CommandError::InvalidInput(format!(
            "deleting a '{operator}' text operator is not supported"
        )));
    }

    operations.remove(idx);
    let new_content = Content { operations }.encode().map_err(lopdf_err)?;
    doc.change_page_content(page_id, new_content).map_err(lopdf_err)?;

    let mut out = Vec::new();
    doc.save_to(&mut out)
        .map_err(|e| CommandError::PdfError(format!("lopdf save: {e}")))?;
    Ok(out)
}

/// SPEC: P4-EDIT-004 — delete a run as one undoable edit. Same swap-and-snapshot
/// shape as [`ReplaceTextRunEdit`]; the inverse restores the pre-delete bytes.
pub struct DeleteTextRunEdit {
    pub page: usize,
    pub run_index: usize,
}

impl<'a> Edit<PdfDocument<'a>> for DeleteTextRunEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        let pre_bytes = {
            let _guard = pdfium_lock()?;
            doc.save_to_bytes().map_err(CommandError::from)?
        };
        let new_bytes = delete_text_run(&pre_bytes, self.page, self.run_index)?;
        {
            let _guard = pdfium_lock()?;
            *doc = pdfium()?
                .load_pdf_from_byte_vec(new_bytes, None)
                .map_err(CommandError::from)?;
        }
        Ok(Box::new(RestoreDocEdit { bytes: pre_bytes }))
    }

    fn label(&self) -> &'static str {
        "delete-text-run"
    }
}
