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
use crate::pdf::font_embed_cid::{build_cid_font, cid_run_fragment, CidRun};
use crate::pdf::font_resolver::covering_font_bytes;
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
    /// SPEC: P7-OCR-002 — a system face covering this page's non-WinAnsi words
    /// (Cyrillic, CJK, Arabic, Devanagari…), found once per page. `None` when
    /// the page is plain Latin, or when no installed face covers it — in which
    /// case those words were dropped rather than written as `.notdef` boxes.
    embed_font: Option<Vec<u8>>,
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
            embed_font: None,
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
fn place_words(found: &Recognised, vw: f32, vh: f32) -> Vec<Placed> {
    let (rw, rh) = (found.render_width as f32, found.render_height as f32);
    if rw <= 0.0 || rh <= 0.0 {
        return Vec::new();
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
    for word in &found.ocr.words {
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
    placed
}

/// One `BT … Tj … ET` for a word the base-14 font can encode.
fn base14_word(font: &str, base: &str, word: &Placed) -> String {
    use std::fmt::Write as _;
    let mut content = String::new();
    let words = std::slice::from_ref(word);
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
    content
}

/// SPEC: P7-OCR-001 — write the recognised words into `bytes` as invisible
/// text, one content fragment per page.
///
/// Words the base-14 font can encode go through the cheap path; the rest ride
/// the CID path (SPEC: P7-OCR-002), which shares the embedding machinery the
/// header/footer writer uses. Both sit inside the same `3 Tr` block: text
/// render mode is graphics state, and `q` inherits it, so the embedded run is
/// invisible for exactly the same reason the Latin one is.
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
        let words = place_words(page, vw, vh);
        if words.is_empty() {
            continue;
        }

        // Which fonts this page needs. Both are registered up front so the
        // words themselves can be emitted in the order Tesseract read them:
        // writing all the Latin words and then all the others would put a
        // page's text in the wrong order for every reader, because extraction
        // follows the content stream. (Measured 2026-09-22 on the mixed
        // Cyrillic fixture: PDF.js read "4821 14 Счёт номер документ от марта".)
        let needs_base = words.iter().any(|w| winansi_fits(&w.text));
        let needs_cid = words.iter().any(|w| !winansi_fits(&w.text));

        let mut content = String::new();
        // `3 Tr` once for the whole block: every glyph below is laid out and
        // selectable, and none of them is painted.
        let _ = std::fmt::Write::write_fmt(
            &mut content,
            format_args!("q\n{}\n3 Tr\n", visual_cm_line(vt)),
        );

        let base_name = if needs_base {
            Some(register_page_resource(
                &mut doc,
                page_id,
                b"Font",
                "Focr",
                Object::Dictionary(base14_font_dict(base)),
            )?)
        } else {
            None
        };

        let cid = if needs_cid {
            let Some(font_bytes) = page.embed_font.as_ref() else {
                return Err(CommandError::Internal(
                    "non-Latin words survived without an embedding font; prepare_words should have dropped them".into(),
                ));
            };
            let text: String = words
                .iter()
                .filter(|w| !winansi_fits(&w.text))
                .map(|w| w.text.as_str())
                .collect();
            let font = build_cid_font(&mut doc, font_bytes, &text)?;
            let name = register_page_resource(
                &mut doc,
                page_id,
                b"Font",
                "Focrcid",
                Object::Dictionary(font.font_dict.clone()),
            )?;
            Some((font, name))
        } else {
            None
        };

        for word in &words {
            match (winansi_fits(&word.text), base_name.as_ref(), cid.as_ref()) {
                (true, Some(font_name), _) => {
                    content.push_str(&base14_word(font_name, base, word));
                }
                (false, _, Some((font, font_name))) => {
                    // The run's own matrix carries position, slant and the
                    // horizontal squeeze that makes the selectable run as wide
                    // as the ink; `cid_run_fragment` emits no `Tz`.
                    let size = (word.height * 0.95).max(1.0);
                    let natural = font.width(&word.text, size);
                    let stretch = if natural > 0.0 { word.width / natural } else { 1.0 };
                    let (sin, cos) = word.degrees.to_radians().sin_cos();
                    let run = CidRun {
                        text: &word.text,
                        size,
                        color: (0.0, 0.0, 0.0),
                        matrix: [
                            cos * stretch,
                            sin * stretch,
                            -sin,
                            cos,
                            word.x,
                            word.y + word.height * 0.18,
                        ],
                        opacity: 1.0,
                        behind: false,
                        underline: None,
                        kind: "ocr-text",
                    };
                    content.push_str(&cid_run_fragment(&mut doc, page_id, font_name, font, &run)?);
                }
                _ => {}
            }
        }

        content.push_str("Q\n");
        written += u32::try_from(words.len()).unwrap_or(u32::MAX);
        append_page_content(&mut doc, page_id, wrap_decoration("ocr-text", content))?;
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

/// Decide what will actually be written, and say how much was dropped.
///
/// Two reasons a recognised word does not make it: the engine was not sure
/// enough (`min_confidence`), or nothing on this machine can draw it. The
/// second only applies to text outside `WinAnsi` — Cyrillic, CJK, Arabic,
/// Devanagari — which needs an embedded face; one covering face is resolved per
/// page here, so the write itself has no decisions left to make.
///
/// SPEC: P7-OCR-002 — without this, the eleven non-Latin languages would
/// recognise perfectly and write nothing at all.
pub fn prepare_words(found: &mut [Recognised], min_confidence: f32) -> (u32, u32) {
    let mut kept = 0u32;
    let mut skipped = 0u32;
    for page in found.iter_mut() {
        let before = page.ocr.words.len();
        page.ocr.words.retain(|w| w.confidence >= min_confidence);
        skipped += u32::try_from(before - page.ocr.words.len()).unwrap_or(u32::MAX);

        let needs_embedding: String = page
            .ocr
            .words
            .iter()
            .filter(|w| !winansi_fits(&w.text))
            .map(|w| w.text.as_str())
            .collect();
        if !needs_embedding.is_empty() {
            page.embed_font = covering_font_bytes(&needs_embedding);
            if page.embed_font.is_none() {
                // Honest degradation: drop them and count them, rather than
                // writing boxes a reader would happily "find".
                let before = page.ocr.words.len();
                page.ocr.words.retain(|w| winansi_fits(&w.text));
                skipped += u32::try_from(before - page.ocr.words.len()).unwrap_or(u32::MAX);
            }
        }
        kept += u32::try_from(page.ocr.words.len()).unwrap_or(u32::MAX);
    }
    (kept, skipped)
}

#[allow(clippy::expect_used, clippy::unwrap_used)]
#[cfg(test)]
mod fixture_builder {
    //! Rebuilds `tests/fixtures/basic/scan-cyrillic.pdf`: Cyrillic set in an
    //! embedded system face, rasterised, and re-embedded as a picture — a
    //! Russian page with nothing selectable in it.
    //!
    //! Here rather than in an integration test because building it needs the
    //! crate-private CID embedding: Ghostscript's base fonts (which the Python
    //! fixture generators drive) cannot set Cyrillic at all.

    use lopdf::{Dictionary, Document, Object, Stream};

    use crate::pdf::document::pdfium;
    use crate::pdf::font_embed_cid::{build_cid_font, place_cid_run, CidRun};
    use crate::pdf::font_resolver::covering_font_bytes;
    use crate::pdf::render::{render_page, ImageFormat};

    const LINES: &[(f32, &str)] = &[
        (24.0, "Счёт номер 4821"),
        (12.0, "документ от 14 марта"),
    ];
    const SCAN_DPI: f32 = 150.0;
    const PAGE: (f32, f32) = (612.0, 792.0);

    fn blank_page_doc() -> (Document, lopdf::ObjectId) {
        let mut doc = Document::with_version("1.5");
        let tree_id = doc.new_object_id();
        let leaf_id = doc.new_object_id();
        let mut page = Dictionary::new();
        page.set("Type", Object::Name(b"Page".to_vec()));
        page.set("Parent", Object::Reference(tree_id));
        page.set(
            "MediaBox",
            Object::Array(vec![0.into(), 0.into(), Object::Real(PAGE.0), Object::Real(PAGE.1)]),
        );
        page.set("Resources", Object::Dictionary(Dictionary::new()));
        doc.objects.insert(leaf_id, Object::Dictionary(page));

        let mut pages = Dictionary::new();
        pages.set("Type", Object::Name(b"Pages".to_vec()));
        pages.set("Kids", Object::Array(vec![Object::Reference(leaf_id)]));
        pages.set("Count", 1);
        doc.objects.insert(tree_id, Object::Dictionary(pages));

        let mut catalog = Dictionary::new();
        catalog.set("Type", Object::Name(b"Catalog".to_vec()));
        catalog.set("Pages", Object::Reference(tree_id));
        let catalog_id = doc.add_object(Object::Dictionary(catalog));
        doc.trailer.set("Root", Object::Reference(catalog_id));
        (doc, leaf_id)
    }

    #[test]
    #[ignore = "regenerates a committed fixture; run on demand"]
    fn writes_the_cyrillic_scan_fixture() {
        let text: String = LINES.iter().map(|(_, t)| *t).collect();
        let font_bytes =
            covering_font_bytes(&text).expect("a system face covering Cyrillic is needed");

        // 1. Cyrillic as real text, in an embedded face.
        let (mut doc, page_id) = blank_page_doc();
        let cid = build_cid_font(&mut doc, &font_bytes, &text).expect("embed");
        let font_name = crate::pdf::cos::register_page_resource(
            &mut doc,
            page_id,
            b"Font",
            "Fcy",
            Object::Dictionary(cid.font_dict.clone()),
        )
        .expect("register");
        let mut y = PAGE.1 - 120.0;
        for (size, line) in LINES {
            place_cid_run(
                &mut doc,
                page_id,
                &font_name,
                &cid,
                &CidRun {
                    text: line,
                    size: *size,
                    color: (0.0, 0.0, 0.0),
                    matrix: [1.0, 0.0, 0.0, 1.0, 72.0, y],
                    opacity: 1.0,
                    behind: false,
                    underline: None,
                    kind: "fixture",
                },
            )
            .expect("place");
            y -= size * 2.5;
        }
        let mut text_pdf = Vec::new();
        doc.save_to(&mut text_pdf).expect("save text pdf");

        // 2. Rasterise it — this is the "scanner".
        let engine = pdfium().expect("pdfium");
        let rendered = {
            let loaded = engine.load_pdf_from_byte_vec(text_pdf, None).expect("load");
            render_page(&loaded, 0, SCAN_DPI, ImageFormat::Rgba8).expect("render")
        };
        let gray: Vec<u8> = rendered
            .bytes
            .chunks_exact(4)
            .map(|p| {
                let v = 0.299 * f32::from(p[0]) + 0.587 * f32::from(p[1]) + 0.114 * f32::from(p[2]);
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let byte = v.round().clamp(0.0, 255.0) as u8;
                byte
            })
            .collect();

        // 3. Put the raster back as the page's only content.
        let (mut out, page_id) = blank_page_doc();
        let mut image = Dictionary::new();
        image.set("Type", Object::Name(b"XObject".to_vec()));
        image.set("Subtype", Object::Name(b"Image".to_vec()));
        image.set("Width", i64::from(rendered.width));
        image.set("Height", i64::from(rendered.height));
        image.set("ColorSpace", Object::Name(b"DeviceGray".to_vec()));
        image.set("BitsPerComponent", 8);
        let mut stream = Stream::new(image, gray);
        stream.compress().expect("compress");
        let image_id = out.add_object(stream);
        let name = crate::pdf::cos::register_page_resource(
            &mut out,
            page_id,
            b"XObject",
            "Im",
            Object::Reference(image_id),
        )
        .expect("register image");
        crate::pdf::cos::append_page_content(
            &mut out,
            page_id,
            format!("q {} 0 0 {} 0 0 cm /{name} Do Q", PAGE.0, PAGE.1),
        )
        .expect("content");

        let target = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/fixtures/basic/scan-cyrillic.pdf");
        let mut bytes = Vec::new();
        out.save_to(&mut bytes).expect("save");
        std::fs::write(&target, &bytes).expect("write fixture");
        println!("wrote {} ({} bytes)", target.display(), bytes.len());
    }
}
