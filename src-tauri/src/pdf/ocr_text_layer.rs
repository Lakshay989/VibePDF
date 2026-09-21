//! SPEC: P7-OCR-001 — turn a scanned page into a searchable one by writing the
//! words Tesseract read as *invisible* text on top of the picture of them.
//!
//! The mechanism is PDF text render mode 3 (`3 Tr`): the glyphs are laid out,
//! measured and selectable, but never painted. A reader's search and text
//! selection see them; the eye sees only the scan. That is what "searchable
//! PDF" means everywhere else, and why the page must look bit-for-bit
//! unchanged afterwards.
//!
//! Placement is the whole job, and it goes through three spaces:
//!
//! 1. **Processed image pixels** — where Tesseract's boxes live, after
//!    [`crate::ocr::preprocess`] has upscaled and (maybe) straightened the
//!    render. Origin top-left, y down.
//! 2. **Render pixels** — undo the upscale and the deskew rotation, giving the
//!    position on the page image as `PDFium` drew it.
//! 3. **Visual points** — scale by the displayed box and flip y. Writing in
//!    visual space means `/Rotate` and `CropBox` are handled by the same
//!    `visual_transform` prefix that watermarks and headers use, rather than by
//!    new geometry here.
//!
//! A deskewed page keeps its tilt in the output: the words are drawn along the
//! angle they sit at in the scan, via a rotated text matrix. Straightening for
//! recognition must not move the text away from the ink it belongs to.
//!
//! Writing goes through lopdf on the serialized bytes, wrapped in the actor's
//! snapshot → reload chassis, exactly like `header_footer`. Two consequences
//! come free: a password-protected document is refused (`P1-VIEW-003`), and a
//! signed one is saved as an incremental update.

use lopdf::{Document, Object, ObjectId};
use pdfium_render::prelude::PdfDocument;
use serde::Serialize;

use crate::error::CommandError;
use crate::ocr::engine::OcrEngine;
use crate::ocr::preprocess::{preprocess_reported, GrayImage, PreprocessOptions, PreprocessReport};
use crate::ocr::OcrPage;
use crate::pdf::cos::{
    append_page_content, base14_font_dict, base_font, escape_pdf_string, page_effective_box,
    page_rotation, register_page_resource, visual_cm_line, visual_transform, winansi_fits,
    wrap_decoration,
};
use crate::pdf::document::{pdfium_lock, replace_with_edited_bytes};
use crate::pdf::render::{render_page, ImageFormat};
use crate::pdf::restore::RestoreDocEdit;
use crate::pdf::undo::Edit;

/// How OCR runs over a document.
#[derive(Debug, Clone, PartialEq)]
pub struct OcrOptions {
    /// Tesseract language, e.g. `"eng"`.
    pub language: String,
    /// Render resolution. 300 is the floor Tesseract is happiest at and what
    /// P7-OCR-003 upscales towards; rendering there directly beats rendering
    /// low and interpolating up.
    pub dpi: f32,
    /// Words the engine is less sure of than this are left out. Invisible text
    /// that is wrong is worse than absent: search silently lies about a page.
    pub min_confidence: f32,
    pub preprocess: PreprocessOptions,
}

impl Default for OcrOptions {
    fn default() -> Self {
        Self {
            language: "eng".into(),
            dpi: 300.0,
            min_confidence: 0.5,
            preprocess: PreprocessOptions::default(),
        }
    }
}

/// What a run did, for the caller and the UI that will exist in a later step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrSummary {
    pub pages: u32,
    /// Words written as invisible text.
    pub words: u32,
    /// Words recognised but dropped: below `min_confidence`, or carrying
    /// characters the base font cannot encode (other scripts arrive in P7.A3).
    pub skipped: u32,
    pub milliseconds: u64,
}

/// One page's recognition, with everything needed to map its boxes back.
pub struct Recognised {
    page: usize,
    ocr: OcrPage,
    report: PreprocessReport,
    /// Size of the render the OCR boxes ultimately refer to.
    render_width: u32,
    render_height: u32,
}

/// Render, prepare and recognise each page. Reads the document; writes nothing,
/// so the caller can count what was found before deciding to write it.
///
/// # Errors
/// When the language data is missing, a page fails to render, or Tesseract
/// refuses the image.
pub fn recognise_pages(
    doc: &PdfDocument<'_>,
    pages: &[usize],
    options: &OcrOptions,
) -> Result<Vec<Recognised>, CommandError> {
    let engine = OcrEngine::new(&options.language)?;
    let mut out = Vec::with_capacity(pages.len());
    for &page in pages {
        let index = u32::try_from(page)
            .map_err(|_| CommandError::InvalidInput(format!("bad page index: {page}")))?;
        // `render_page` takes the PDFium lock itself, so none is held here.
        let rendered = render_page(doc, index, options.dpi, ImageFormat::Rgba8)?;
        let gray = GrayImage::from_rgba(rendered.width, rendered.height, &rendered.bytes)
            .map_err(|e| CommandError::Internal(format!("page {page}: {e}")))?;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let source_dpi = options.dpi.max(1.0) as u32;
        let (prepared, report) = preprocess_reported(&gray, source_dpi, options.preprocess)
            .map_err(|e| CommandError::Internal(format!("page {page}: {e}")))?;
        out.push(Recognised {
            page,
            ocr: engine.recognize(&prepared)?,
            report,
            render_width: rendered.width,
            render_height: rendered.height,
        });
    }
    Ok(out)
}

/// A word placed in a page's visual space: where it starts, how wide and tall
/// it is, and the angle its baseline runs at.
struct Placed {
    text: String,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    degrees: f32,
}

/// Map one recognised page's words into visual-space placements.
#[allow(clippy::cast_precision_loss)]
fn place_words(found: &Recognised, vw: f32, vh: f32, min_confidence: f32) -> (Vec<Placed>, u32) {
    let (rw, rh) = (found.render_width as f32, found.render_height as f32);
    if rw <= 0.0 || rh <= 0.0 {
        return (Vec::new(), 0);
    }
    // Undo the upscale, then the straightening, then scale into points.
    #[allow(clippy::cast_possible_truncation)] // a resize ratio, near 1
    let scale = if found.report.scale > 0.0 {
        found.report.scale as f32
    } else {
        1.0
    };
    let skew = found.report.skew_degrees;
    let (centre_x, centre_y) = (rw / 2.0, rh / 2.0);
    let (sin, cos) = (-skew).to_radians().sin_cos();

    let mut placed = Vec::new();
    let mut skipped = 0;
    for word in &found.ocr.words {
        if word.confidence < min_confidence || !winansi_fits(&word.text) {
            skipped += 1;
            continue;
        }
        let [left, top, right, bottom] = word.rect;
        // Bottom-left of the box, in render pixels.
        let (px, py) = (left / scale, bottom / scale);
        let (dx, dy) = (px - centre_x, py - centre_y);
        let rx = dx * cos + dy * sin + centre_x;
        let ry = -dx * sin + dy * cos + centre_y;

        placed.push(Placed {
            text: word.text.clone(),
            x: rx * vw / rw,
            // Image y runs down, visual y runs up.
            y: vh - ry * vh / rh,
            width: (right - left) / scale * vw / rw,
            height: (bottom - top) / scale * vh / rh,
            // The estimate is in image space, where y runs *down*; PDF text
            // space has y up, so the same slant is the opposite sign. Measured
            // 2026-09-21 against a 3-degree scan: `skew` put the words at -3.
            degrees: -skew,
        });
    }
    (placed, skipped)
}

/// The `q … Q` fragment drawing `words` invisibly, in visual coordinates.
fn text_layer_content(font: &str, base: &str, vt: [f32; 6], words: &[Placed]) -> String {
    use std::fmt::Write as _;
    let mut content = String::new();
    // `3 Tr` once for the whole block: every glyph below is laid out and
    // selectable, and none of them is painted.
    let _ = writeln!(content, "q\n{}\n3 Tr", visual_cm_line(vt));
    for word in words {
        // Tesseract's box hugs the glyphs, so its height is close to the font
        // size, and the baseline sits a little above the bottom of anything
        // with a descender.
        let size = (word.height * 0.95).max(1.0);
        let baseline = word.y + word.height * 0.18;
        // Squeeze or stretch the glyphs so the selectable run is as wide as the
        // ink it covers; without this, selecting a word highlights the wrong
        // span and readers join words wrongly when extracting.
        let natural = crate::pdf::font_metrics::text_width(base, &word.text, size);
        let stretch = if natural > 0.0 {
            (word.width / natural * 100.0).clamp(10.0, 1000.0)
        } else {
            100.0
        };
        let (sin, cos) = word.degrees.to_radians().sin_cos();
        let _ = writeln!(
            content,
            "BT\n/{font} {size:.2} Tf\n{stretch:.2} Tz\n{cos:.5} {sin:.5} {nsin:.5} {cos:.5} {x:.2} {baseline:.2} Tm\n({esc}) Tj\nET",
            nsin = -sin,
            x = word.x,
            esc = escape_pdf_string(&word.text),
        );
    }
    content.push_str("Q\n");
    content
}

/// SPEC: P7-OCR-001 — write the recognised words into `bytes` as invisible
/// text, one content fragment per page.
fn write_text_layer(bytes: &[u8], found: &[Recognised]) -> Result<(Vec<u8>, u32), CommandError> {
    let mut doc = Document::load_mem(bytes).map_err(|e| CommandError::PdfError(e.to_string()))?;
    let base = base_font("Helvetica", false, false)?;
    let page_map = doc.get_pages();
    let mut written = 0;

    for page in found {
        let page_no = u32::try_from(page.page)
            .ok()
            .map(|n| n + 1)
            .ok_or_else(|| CommandError::InvalidInput(format!("bad page index: {}", page.page)))?;
        let page_id: ObjectId = *page_map
            .get(&page_no)
            .ok_or_else(|| CommandError::InvalidInput(format!("page out of range: {}", page.page)))?;

        let rotate = page_rotation(&doc, page_id);
        let (vt, vw, vh) = visual_transform(rotate, page_effective_box(&doc, page_id));
        let (words, _) = place_words(page, vw, vh, 0.0); // already filtered
        if words.is_empty() {
            continue;
        }
        let font_name = register_page_resource(
            &mut doc,
            page_id,
            b"Font",
            "Focr",
            Object::Dictionary(base14_font_dict(base)),
        )?;
        written += u32::try_from(words.len()).unwrap_or(u32::MAX);
        append_page_content(
            &mut doc,
            page_id,
            wrap_decoration("ocr-text", text_layer_content(&font_name, base, vt, &words)),
        )?;
    }

    let mut buf = Vec::new();
    doc.save_to(&mut buf)?;
    Ok((buf, written))
}

/// SPEC: P7-OCR-001 — write the invisible text layer for already-recognised
/// pages, as one undoable edit. The inverse is a pre-write snapshot, like every
/// other bytes → bytes decoration.
///
/// Recognition happens before the edit ([`recognise_pages`]) so the actor can
/// report what was found, and so a page that reads as nothing costs no edit.
pub struct OcrTextLayerEdit {
    pub found: Vec<Recognised>,
}

impl<'a> Edit<PdfDocument<'a>> for OcrTextLayerEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        let pre_bytes = {
            let _guard = pdfium_lock()?;
            doc.save_to_bytes().map_err(CommandError::from)?
        };
        let (new_bytes, _written) = write_text_layer(&pre_bytes, &self.found)?;
        {
            let _guard = pdfium_lock()?;
            replace_with_edited_bytes(doc, new_bytes)?;
        }
        Ok(Box::new(RestoreDocEdit { bytes: pre_bytes }))
    }

    fn label(&self) -> &'static str {
        "ocr-text-layer"
    }
}

/// Drop the words that must not be written, and say how many went: below
/// `min_confidence`, or carrying characters the base font cannot encode (other
/// scripts arrive with P7.A3's language packs).
pub fn keep_writable_words(found: &mut [Recognised], min_confidence: f32) -> (u32, u32) {
    let mut kept = 0u32;
    let mut skipped = 0u32;
    for page in found {
        let before = page.ocr.words.len();
        page.ocr
            .words
            .retain(|w| w.confidence >= min_confidence && winansi_fits(&w.text));
        kept += u32::try_from(page.ocr.words.len()).unwrap_or(u32::MAX);
        skipped += u32::try_from(before - page.ocr.words.len()).unwrap_or(u32::MAX);
    }
    (kept, skipped)
}
