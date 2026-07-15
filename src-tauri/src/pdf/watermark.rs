//! Watermark (P4.D2) — stamp text or an image across selected pages.
//!
//! SPEC: P4-EDIT-009. A watermark is **page content**, not an annotation: for
//! each selected page we register an opacity `/ExtGState` plus a font (text) or
//! Image `XObject` (image), then add a `q … Q`-balanced content fragment that
//! draws the mark rotated about, and centred on, the page centre. Placing it
//! *on top* appends the stream ([`crate::pdf::cos::append_page_content`]); placing
//! it *behind* prepends it ([`crate::pdf::cos::prepend_page_content`]). Pure-lopdf
//! object adds + one save, so 50 pages stay well under the 2 s budget.
//!
//! The whole transform is bytes → bytes; the actor wraps it in the snapshot →
//! reload chassis ([`watermark_apply`]), inverse `RestoreDocEdit`, just like
//! `image_edit`.

use lopdf::{Dictionary, Document, Object, ObjectId};
use pdfium_render::prelude::PdfDocument;

use crate::error::CommandError;
use crate::pdf::cos::{
    append_page_content, base_font, compose, escape_pdf_string, font_avg_em, page_effective_box,
    page_rotation, parse_hex_color, prepend_page_content, register_page_resource, visual_cm_line,
    visual_transform, wrap_decoration,
};
use crate::pdf::document::{pdfium, pdfium_lock};
use crate::pdf::font_embed::{embed_runs, EmbedRun};
use crate::pdf::image_xobject::embed_image;
use crate::pdf::restore::RestoreDocEdit;
use crate::pdf::undo::Edit;

/// What to stamp: a text string (in a base-14 font + colour) or a raster image.
pub enum WatermarkKind {
    Text {
        text: String,
        font_family: String,
        size: f32,
        color: String,
        bold: bool,
        italic: bool,
    },
    /// Raw PNG/JPEG bytes (read by the command; the actor stays byte-pure).
    Image(Vec<u8>),
}

/// SPEC: P4-EDIT-009 — stamp `kind` on each 0-based page in `pages`, at `opacity`
/// (0..1) and `rotation_deg`, centred on each page. `behind` draws it under the
/// existing content; otherwise on top.
pub fn add_watermark(
    bytes: &[u8],
    pages: &[usize],
    kind: &WatermarkKind,
    opacity: f32,
    rotation_deg: f32,
    behind: bool,
) -> Result<Vec<u8>, CommandError> {
    if pages.is_empty() {
        return Err(CommandError::InvalidInput("no pages selected for watermark".into()));
    }
    if let WatermarkKind::Text { text, .. } = kind {
        if text.trim().is_empty() {
            return Err(CommandError::InvalidInput("watermark text is empty".into()));
        }
        // FABLE_REVIEW 3.2 stage-2 (P4.HF7): non-WinAnsi watermark text embeds a
        // covering system font via PDFium; WinAnsi text keeps the base-14 path.
        if !crate::pdf::cos::winansi_fits(text) {
            let Some(font_bytes) = crate::pdf::font_resolver::covering_font_bytes(text) else {
                // No embeddable face — fall back to the honest HF3 rejection.
                crate::pdf::cos::ensure_winansi(text)?;
                return Err(CommandError::Internal(
                    "non-WinAnsi watermark text unexpectedly passed the WinAnsi check".into(),
                ));
            };
            return add_watermark_embedded(
                bytes,
                pages,
                kind,
                opacity,
                rotation_deg,
                behind,
                &font_bytes,
            );
        }
    }
    let opacity = opacity.clamp(0.0, 1.0);
    let theta = rotation_deg.to_radians();
    let (cos, sin) = (theta.cos(), theta.sin());

    let mut doc = Document::load_mem(bytes).map_err(cos_err)?;

    // Resolve + range-check every target page before mutating, so a bad index
    // fails cleanly with nothing half-written.
    let page_map = doc.get_pages();
    let mut targets: Vec<ObjectId> = Vec::with_capacity(pages.len());
    for &p in pages {
        let page_no = u32::try_from(p)
            .ok()
            .map(|n| n + 1)
            .ok_or_else(|| CommandError::InvalidInput(format!("bad page index: {p}")))?;
        let id = *page_map
            .get(&page_no)
            .ok_or_else(|| CommandError::InvalidInput(format!("page index out of range: {p}")))?;
        targets.push(id);
    }

    // An image watermark is embedded once and referenced from every page.
    let image = match kind {
        WatermarkKind::Image(data) => Some(embed_image(&mut doc, data)?),
        WatermarkKind::Text { .. } => None,
    };

    for page_id in targets {
        // Lay out in VISUAL space (displayed CropBox, after /Rotate) so the
        // mark reads upright and centred on what the user actually sees.
        let rotate = page_rotation(&doc, page_id);
        let (vt, vw, vh) = visual_transform(rotate, page_effective_box(&doc, page_id));
        let (cx, cy) = (vw / 2.0, vh / 2.0);

        let mut gs = Dictionary::new();
        gs.set("Type", Object::Name(b"ExtGState".to_vec()));
        gs.set("ca", Object::Real(opacity));
        gs.set("CA", Object::Real(opacity));
        gs.set("BM", Object::Name(b"Normal".to_vec()));
        let gs_name = register_page_resource(&mut doc, page_id, b"ExtGState", "GSwm", Object::Dictionary(gs))?;

        let content = match kind {
            WatermarkKind::Text { text, font_family, size, color, bold, italic } => {
                let base = base_font(font_family, *bold, *italic)?;
                let font_name =
                    register_page_resource(&mut doc, page_id, b"Font", "Fwm", font_dict(base))?;
                let rgb = parse_hex_color(color)?;
                let sz = size.max(1.0);
                #[allow(clippy::cast_precision_loss)]
                let width = sz * font_avg_em(base) * text.chars().count() as f32;
                text_content(&gs_name, &font_name, rgb, sz, text, width, (cos, sin), (cx, cy), vt)
            }
            WatermarkKind::Image(_) => {
                let img = image
                    .as_ref()
                    .ok_or_else(|| CommandError::Internal("watermark image not embedded".into()))?;
                let name = register_page_resource(
                    &mut doc,
                    page_id,
                    b"XObject",
                    "Imgwm",
                    Object::Reference(img.id),
                )?;
                let (sw, sh) = fit_dims(img.width, img.height, vw * 0.7, vh * 0.7);
                image_content(&gs_name, &name, (sw, sh), (cos, sin), (cx, cy), vt)
            }
        };

        let content = wrap_decoration("watermark", content);
        if behind {
            prepend_page_content(&mut doc, page_id, content)?;
        } else {
            append_page_content(&mut doc, page_id, content)?;
        }
    }

    let mut buf = Vec::new();
    doc.save_to(&mut buf)?;
    Ok(buf)
}

/// The non-WinAnsi text-watermark path (`FABLE_REVIEW` 3.2 stage-2): place the
/// mark as a `PDFium` text object drawn with an embedded, subsetted covering font.
/// Geometry (rotation, effective box) comes from an lopdf pass; the placement
/// matrix bakes the *same* `vt · R@centre · Td(-w/2,-size/3)` transform the base-14
/// `text_content` applies, so rotated/cropped pages still land centred and upright.
/// `opacity` becomes the object's fill alpha and `behind` inserts it under the
/// page content — the two watermark features the base-14 path did with an
/// `/ExtGState` + `prepend`.
///
/// Known gaps vs. the base-14 path (shared with header/footer): no HF2 marked-
/// content tag on the run, and centring uses the base-14 width estimate (3.10).
#[allow(clippy::too_many_arguments, clippy::many_single_char_names)]
fn add_watermark_embedded(
    bytes: &[u8],
    pages: &[usize],
    kind: &WatermarkKind,
    opacity: f32,
    rotation_deg: f32,
    behind: bool,
    font_bytes: &[u8],
) -> Result<Vec<u8>, CommandError> {
    let WatermarkKind::Text { text, font_family, size, color, bold, italic } = kind else {
        return Err(CommandError::Internal("embedded watermark path requires text".into()));
    };
    let base = base_font(font_family, *bold, *italic)?;
    let rgb = parse_hex_color(color)?;
    let sz = size.max(1.0);
    let opacity = opacity.clamp(0.0, 1.0);
    let theta = rotation_deg.to_radians();
    let (cos, sin) = (theta.cos(), theta.sin());
    #[allow(clippy::cast_precision_loss)]
    let width = sz * font_avg_em(base) * text.chars().count() as f32;
    let (tx, ty) = (-width / 2.0, -sz / 3.0);

    let doc = Document::load_mem(bytes).map_err(cos_err)?;
    let page_map = doc.get_pages();
    let mut runs: Vec<EmbedRun> = Vec::with_capacity(pages.len());
    for &p in pages {
        let page_no = u32::try_from(p)
            .ok()
            .map(|n| n + 1)
            .ok_or_else(|| CommandError::InvalidInput(format!("bad page index: {p}")))?;
        let page_id = *page_map
            .get(&page_no)
            .ok_or_else(|| CommandError::InvalidInput(format!("page index out of range: {p}")))?;
        let rotate = page_rotation(&doc, page_id);
        let (vt, vw, vh) = visual_transform(rotate, page_effective_box(&doc, page_id));
        let (cx, cy) = (vw / 2.0, vh / 2.0);
        // Glyph maps as p · T · R · vt (Td, then rotate-about-centre, then visual).
        let r_mat = [cos, sin, -sin, cos, cx, cy];
        let t_mat = [1.0, 0.0, 0.0, 1.0, tx, ty];
        runs.push(EmbedRun {
            page: p,
            text: text.clone(),
            size: sz,
            color: rgb,
            opacity,
            matrix: compose(compose(t_mat, r_mat), vt),
            behind,
        });
    }
    embed_runs(bytes, font_bytes, &runs)
}

#[allow(clippy::needless_pass_by_value)]
fn cos_err(e: lopdf::Error) -> CommandError {
    CommandError::PdfError(format!("lopdf: {e}"))
}

/// Aspect-fit `iw`×`ih` into a `bw`×`bh` box (never stretched), returning the
/// drawn width/height.
#[allow(clippy::cast_precision_loss)]
fn fit_dims(iw: u32, ih: u32, bw: f32, bh: f32) -> (f32, f32) {
    let aspect = iw as f32 / ih as f32;
    if aspect > bw / bh {
        (bw, bw / aspect)
    } else {
        (bh * aspect, bh)
    }
}

fn font_dict(base: &str) -> Object {
    Object::Dictionary(crate::pdf::cos::base14_font_dict(base))
}

/// `q … Q` fragment: map visual → page space (`vt`), rotate about the visual
/// centre (`cm`), then draw the centred text in `rgb` at `size`. `width` (the
/// estimated text advance) centres it.
#[allow(clippy::many_single_char_names, clippy::too_many_arguments)]
fn text_content(
    gs: &str,
    font: &str,
    rgb: (f32, f32, f32),
    size: f32,
    text: &str,
    width: f32,
    (cos, sin): (f32, f32),
    (cx, cy): (f32, f32),
    vt: [f32; 6],
) -> String {
    let (r, g, b) = rgb;
    format!(
        "q\n/{gs} gs\n{r:.4} {g:.4} {b:.4} rg\n{vtl}\n\
         {cos:.5} {sin:.5} {nsin:.5} {cos:.5} {cx:.2} {cy:.2} cm\n\
         BT\n/{font} {size:.2} Tf\n{tx:.2} {ty:.2} Td\n({esc}) Tj\nET\nQ\n",
        vtl = visual_cm_line(vt),
        nsin = -sin,
        tx = -width / 2.0,
        ty = -size / 3.0,
        esc = escape_pdf_string(text),
    )
}

/// `q … Q` fragment: map visual → page space (`vt`), rotate about the visual
/// centre, then paint the image (drawn in the unit square) scaled to `sw`×`sh`
/// and centred on the origin.
fn image_content(
    gs: &str,
    name: &str,
    (sw, sh): (f32, f32),
    (cos, sin): (f32, f32),
    (cx, cy): (f32, f32),
    vt: [f32; 6],
) -> String {
    format!(
        "q\n/{gs} gs\n{vtl}\n\
         {cos:.5} {sin:.5} {nsin:.5} {cos:.5} {cx:.2} {cy:.2} cm\n\
         {sw:.2} 0 0 {sh:.2} {hsw:.2} {hsh:.2} cm\n/{name} Do\nQ\n",
        vtl = visual_cm_line(vt),
        nsin = -sin,
        hsw = -sw / 2.0,
        hsh = -sh / 2.0,
    )
}

/// SPEC: P4-EDIT-009 — apply a watermark as one undoable edit. The inverse is a
/// pre-write snapshot (`RestoreDocEdit`).
pub struct WatermarkEdit {
    pub pages: Vec<i32>,
    pub kind: WatermarkKind,
    pub opacity: f32,
    pub rotation: f32,
    pub behind: bool,
}

impl<'a> Edit<PdfDocument<'a>> for WatermarkEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        let pages: Vec<usize> = self
            .pages
            .iter()
            .map(|&p| {
                usize::try_from(p)
                    .map_err(|_| CommandError::InvalidInput(format!("negative page index: {p}")))
            })
            .collect::<Result<_, _>>()?;
        watermark_apply(doc, |bytes| {
            add_watermark(bytes, &pages, &self.kind, self.opacity, self.rotation, self.behind)
        })
    }

    fn label(&self) -> &'static str {
        "watermark"
    }
}

/// Snapshot the live document, run a bytes → bytes edit, reload/replace it; the
/// inverse is the pre-edit snapshot. (Same shape as `image_edit`'s chassis.)
fn watermark_apply<'a>(
    doc: &mut PdfDocument<'a>,
    f: impl FnOnce(&[u8]) -> Result<Vec<u8>, CommandError>,
) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
    let pre_bytes = {
        let _guard = pdfium_lock()?;
        doc.save_to_bytes().map_err(CommandError::from)?
    };
    let new_bytes = f(&pre_bytes)?;
    {
        let _guard = pdfium_lock()?;
        *doc = pdfium()?
            .load_pdf_from_byte_vec(new_bytes, None)
            .map_err(CommandError::from)?;
    }
    Ok(Box::new(RestoreDocEdit { bytes: pre_bytes }))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use pdfium_render::prelude::*;

    use super::{add_watermark, add_watermark_embedded, WatermarkKind};
    use crate::pdf::cos::compose;
    use crate::pdf::document::{pdfium, pdfium_lock};

    fn hello() -> Vec<u8> {
        std::fs::read("../tests/fixtures/basic/hello.pdf").expect("hello.pdf")
    }

    fn coptic_font() -> Vec<u8> {
        std::fs::read("../tests/fixtures/fonts/NotoSansCoptic-Regular.ttf")
            .expect("committed Coptic fixture font")
    }

    fn text_kind(text: &str) -> WatermarkKind {
        WatermarkKind::Text {
            text: text.to_owned(),
            font_family: "Helvetica".to_owned(),
            size: 48.0,
            color: "#808080".to_owned(),
            bold: false,
            italic: false,
        }
    }

    fn page0_text(bytes: &[u8]) -> String {
        let _guard = pdfium_lock().expect("lock");
        let doc = pdfium()
            .expect("pdfium")
            .load_pdf_from_byte_vec(bytes.to_vec(), None)
            .expect("reopen");
        let page = doc.pages().get(0).expect("page 0");
        let mut out = String::new();
        for object in page.objects().iter() {
            if let Some(t) = object.as_text_object() {
                out.push_str(&t.text());
            }
        }
        drop(doc);
        out
    }

    /// `compose` bakes stacked `cm`s into one matrix: composing with identity is a
    /// no-op, and translate-then-translate adds. (Exact integer-valued arithmetic,
    /// so strict `f32` equality is intentional here.)
    #[test]
    #[allow(clippy::float_cmp)]
    fn compose_matches_stacked_transforms() {
        let id = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
        let m = [2.0, 0.0, 0.0, 3.0, 5.0, 7.0];
        assert_eq!(compose(id, m), m);
        assert_eq!(compose(m, id), m);
        // p·T1·T2 translates by the sum.
        let t1 = [1.0, 0.0, 0.0, 1.0, 10.0, 20.0];
        let t2 = [1.0, 0.0, 0.0, 1.0, 3.0, 4.0];
        assert_eq!(compose(t1, t2), [1.0, 0.0, 0.0, 1.0, 13.0, 24.0]);
    }

    /// P4.HF7: a non-WinAnsi text watermark takes the PDFium-embed path (rotated,
    /// centred) and the Unicode survives reopen alongside the page's base-14 text.
    /// Deterministic — drives the private embedded path with the fixture font.
    #[test]
    fn embedded_watermark_renders_and_extracts_unicode() {
        let coptic = "\u{2C81}\u{2C83}\u{2C85}\u{2C87}"; // Ⲁ Ⲃ Ⲅ Ⲇ
        let out = add_watermark_embedded(
            &hello(),
            &[0],
            &text_kind(coptic),
            0.3,   // translucent
            45.0,  // rotated
            false, // on top
            &coptic_font(),
        )
        .expect("embed watermark");
        let text = page0_text(&out);
        assert!(text.contains(coptic), "embedded Coptic watermark round-trips; got {text:?}");
        assert!(text.contains("VibePDF"), "the page's own base-14 text is intact");
    }

    /// A `behind` embedded watermark draws under the page content (z-index 0).
    #[test]
    fn embedded_watermark_behind_sits_under_content() {
        let coptic = "\u{2C81}\u{2C83}";
        let out = add_watermark_embedded(
            &hello(),
            &[0],
            &text_kind(coptic),
            1.0,
            0.0,
            true, // behind
            &coptic_font(),
        )
        .expect("embed behind watermark");
        let text = page0_text(&out);
        assert!(
            text.find(coptic) < text.find("VibePDF"),
            "behind watermark precedes page content in draw order; got {text:?}",
        );
    }

    /// The `WinAnsi` path is untouched — an ASCII "DRAFT" watermark still produces a
    /// base-14 lopdf content fragment (`Tj` + the `/Fwm` font + opacity `ExtGState`),
    /// not a `PDFium` embed.
    #[test]
    fn winansi_watermark_keeps_base14_path() {
        let out = add_watermark(&hello(), &[0], &text_kind("DRAFT"), 0.5, 45.0, false).expect("wm");
        let doc = lopdf::Document::load_mem(&out).expect("load");
        let page_id = *doc.get_pages().get(&1).expect("page 1");
        let content = String::from_utf8_lossy(&doc.get_page_content(page_id).expect("content")).into_owned();
        assert!(content.contains("(DRAFT) Tj"), "base-14 text show still present");
        assert!(content.contains("/Fwm"), "base-14 watermark font referenced");
    }
}
