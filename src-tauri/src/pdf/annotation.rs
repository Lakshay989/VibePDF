//! Text-markup annotations as an undoable edit (P3.B1b).
//!
//! SPEC: P3-ANN-001 — write a standard PDF text-markup annotation
//! (highlight / underline / strikethrough / squiggly) over selected-text quads.
//! Done via the lopdf COS layer ([`crate::pdf::cos::add_text_markup`]) on a byte
//! round-trip — `PDFium` can't set annotation colour, and lopdf gives full control
//! over `/QuadPoints`, `/C`, and the `/AP` appearance. Same shape as resize:
//! serialize → cos edit → reload, replacing the actor's document. The inverse is
//! a pre-write snapshot ([`RestoreDocEdit`]).

use pdfium_render::prelude::PdfDocument;

use crate::error::CommandError;
use crate::pdf::cos::add_text_markup;
use crate::pdf::document::{pdfium, pdfium_lock};
use crate::pdf::restore::RestoreDocEdit;
use crate::pdf::undo::Edit;

/// Add a text-markup annotation over `quads` (each `[x1..y4]` PDF pts) on `page`.
pub struct TextMarkupEdit {
    pub page: i32,
    pub subtype: String,
    pub quads: Vec<[f32; 8]>,
    pub color: String,
    pub opacity: f32,
}

impl<'a> Edit<PdfDocument<'a>> for TextMarkupEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        if self.quads.is_empty() {
            return Err(CommandError::InvalidInput("no quads for text markup".into()));
        }
        let page = usize::try_from(self.page)
            .map_err(|_| CommandError::InvalidInput(format!("negative page index: {}", self.page)))?;

        // 1. Snapshot the pre-write document — the lossless inverse.
        let pre_bytes = {
            let _guard = pdfium_lock()?;
            doc.save_to_bytes().map_err(CommandError::from)?
        };

        // 2. Write the annotation via lopdf (pure Rust, no lock). Validates the
        //    page + subtype, so a bad request fails here without touching `doc`.
        let new_bytes = add_text_markup(
            &pre_bytes,
            page,
            &self.subtype,
            &self.quads,
            &self.color,
            self.opacity,
        )?;

        // 3. Reload, replacing the actor's document (the reload is the round-trip
        //    verification — a malformed result would fail to open here).
        {
            let _guard = pdfium_lock()?;
            *doc = pdfium()?
                .load_pdf_from_byte_vec(new_bytes, None)
                .map_err(CommandError::from)?;
        }

        Ok(Box::new(RestoreDocEdit { bytes: pre_bytes }))
    }

    fn label(&self) -> &'static str {
        "text-markup"
    }
}
