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
    append_page_content, base_font, escape_pdf_string, font_avg_em, page_media_box, parse_hex_color,
    prepend_page_content, register_page_resource,
};
use crate::pdf::document::{pdfium, pdfium_lock};
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
        let [mx0, my0, mx1, my1] = page_media_box(&doc, page_id);
        let (pw, ph) = (mx1 - mx0, my1 - my0);
        let (cx, cy) = (mx0 + pw / 2.0, my0 + ph / 2.0);

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
                text_content(&gs_name, &font_name, rgb, sz, text, width, (cos, sin), (cx, cy))
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
                let (sw, sh) = fit_dims(img.width, img.height, pw * 0.7, ph * 0.7);
                image_content(&gs_name, &name, (sw, sh), (cos, sin), (cx, cy))
            }
        };

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
    let mut font = Dictionary::new();
    font.set("Type", Object::Name(b"Font".to_vec()));
    font.set("Subtype", Object::Name(b"Type1".to_vec()));
    font.set("BaseFont", Object::Name(base.as_bytes().to_vec()));
    Object::Dictionary(font)
}

/// `q … Q` fragment: rotate about the page centre (`cm`), then draw the centred
/// text in `rgb` at `size`. `width` (the estimated text advance) centres it.
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
) -> String {
    let (r, g, b) = rgb;
    format!(
        "q\n/{gs} gs\n{r:.4} {g:.4} {b:.4} rg\n\
         {cos:.5} {sin:.5} {nsin:.5} {cos:.5} {cx:.2} {cy:.2} cm\n\
         BT\n/{font} {size:.2} Tf\n{tx:.2} {ty:.2} Td\n({esc}) Tj\nET\nQ\n",
        nsin = -sin,
        tx = -width / 2.0,
        ty = -size / 3.0,
        esc = escape_pdf_string(text),
    )
}

/// `q … Q` fragment: rotate about the page centre, then paint the image (drawn
/// in the unit square) scaled to `sw`×`sh` and centred on the origin.
fn image_content(
    gs: &str,
    name: &str,
    (sw, sh): (f32, f32),
    (cos, sin): (f32, f32),
    (cx, cy): (f32, f32),
) -> String {
    format!(
        "q\n/{gs} gs\n\
         {cos:.5} {sin:.5} {nsin:.5} {cos:.5} {cx:.2} {cy:.2} cm\n\
         {sw:.2} 0 0 {sh:.2} {hsw:.2} {hsh:.2} cm\n/{name} Do\nQ\n",
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
