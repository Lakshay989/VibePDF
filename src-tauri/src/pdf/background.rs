//! Background (P4.D1) — fill selected pages with a colour or image *behind* all
//! existing content.
//!
//! SPEC: P4-EDIT-008. The watermark's simpler sibling: a background is always
//! full-page and always **behind**, so it's a single `prepend_page_content`
//! ([`crate::pdf::cos::prepend_page_content`]) of a `q … Q` fragment that fills
//! the page `MediaBox`. Colour paints a filled rectangle; an image is embedded
//! once and drawn **cover-fit with a clip** (fills the page, crops overflow, no
//! distortion). Bytes → bytes; the actor wraps it in the snapshot → reload
//! chassis ([`background_apply`]), inverse `RestoreDocEdit`.
//!
//! The third spec source — a page from another PDF — is deferred to D1b (it needs
//! cross-document page → Form `XObject` import).

use lopdf::{Dictionary, Document, Object, ObjectId};
use pdfium_render::prelude::PdfDocument;

use crate::error::CommandError;
use crate::pdf::cos::{page_media_box, parse_hex_color, prepend_page_content, register_page_resource};
use crate::pdf::document::{pdfium, pdfium_lock};
use crate::pdf::image_xobject::embed_image;
use crate::pdf::restore::RestoreDocEdit;
use crate::pdf::undo::Edit;

/// What to paint behind the page: a solid colour or a raster image.
pub enum BackgroundKind {
    /// `#rrggbb`.
    Color(String),
    /// Raw PNG/JPEG bytes (read by the command; the actor stays byte-pure).
    Image(Vec<u8>),
}

/// SPEC: P4-EDIT-008 — fill each 0-based page in `pages` behind its content with
/// `kind`, at `opacity` (0..1; a background is usually opaque, i.e. 1.0).
#[allow(clippy::many_single_char_names)]
pub fn add_background(
    bytes: &[u8],
    pages: &[usize],
    kind: &BackgroundKind,
    opacity: f32,
) -> Result<Vec<u8>, CommandError> {
    if pages.is_empty() {
        return Err(CommandError::InvalidInput("no pages selected for background".into()));
    }
    let opacity = opacity.clamp(0.0, 1.0);

    let mut doc = Document::load_mem(bytes).map_err(cos_err)?;

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

    // Validate a colour up front (so a bad hex fails before any mutation); embed
    // an image once and reference it from every page.
    let rgb = match kind {
        BackgroundKind::Color(hex) => Some(parse_hex_color(hex)?),
        BackgroundKind::Image(_) => None,
    };
    let image = match kind {
        BackgroundKind::Image(data) => Some(embed_image(&mut doc, data)?),
        BackgroundKind::Color(_) => None,
    };

    for page_id in targets {
        let [x0, y0, x1, y1] = page_media_box(&doc, page_id);
        let (w, h) = (x1 - x0, y1 - y0);

        let mut gs = Dictionary::new();
        gs.set("Type", Object::Name(b"ExtGState".to_vec()));
        gs.set("ca", Object::Real(opacity));
        gs.set("CA", Object::Real(opacity));
        gs.set("BM", Object::Name(b"Normal".to_vec()));
        let gs_name = register_page_resource(&mut doc, page_id, b"ExtGState", "GSbg", Object::Dictionary(gs))?;

        let content = match kind {
            BackgroundKind::Color(_) => {
                let (r, g, b) = rgb.ok_or_else(|| CommandError::Internal("colour not parsed".into()))?;
                color_content(&gs_name, (r, g, b), [x0, y0, w, h])
            }
            BackgroundKind::Image(_) => {
                let img = image
                    .as_ref()
                    .ok_or_else(|| CommandError::Internal("background image not embedded".into()))?;
                let name = register_page_resource(
                    &mut doc,
                    page_id,
                    b"XObject",
                    "Imgbg",
                    Object::Reference(img.id),
                )?;
                image_content(&gs_name, &name, [x0, y0, w, h], img.width, img.height)
            }
        };

        // A background always draws behind the page's own content.
        prepend_page_content(&mut doc, page_id, content)?;
    }

    let mut buf = Vec::new();
    doc.save_to(&mut buf)?;
    Ok(buf)
}

#[allow(clippy::needless_pass_by_value)]
fn cos_err(e: lopdf::Error) -> CommandError {
    CommandError::PdfError(format!("lopdf: {e}"))
}

/// `q … Q`: fill the page rect `[x0, y0, w, h]` with `(r, g, b)`.
#[allow(clippy::many_single_char_names)]
fn color_content(gs: &str, (r, g, b): (f32, f32, f32), [x0, y0, w, h]: [f32; 4]) -> String {
    format!("q\n/{gs} gs\n{r:.4} {g:.4} {b:.4} rg\n{x0:.2} {y0:.2} {w:.2} {h:.2} re\nf\nQ\n")
}

/// `q … Q`: clip to the page rect, then paint the image cover-fit (fills the
/// page, preserves aspect, crops overflow). The image draws in the unit square,
/// so the `cm` scales it to `sw`×`sh` and centres it on the rect.
#[allow(clippy::cast_precision_loss, clippy::many_single_char_names)]
fn image_content(gs: &str, name: &str, [x0, y0, w, h]: [f32; 4], iw: u32, ih: u32) -> String {
    let cover = (w / iw as f32).max(h / ih as f32);
    let (sw, sh) = (iw as f32 * cover, ih as f32 * cover);
    let (tx, ty) = (x0 + (w - sw) / 2.0, y0 + (h - sh) / 2.0);
    format!(
        "q\n/{gs} gs\n{x0:.2} {y0:.2} {w:.2} {h:.2} re\nW n\n\
         {sw:.2} 0 0 {sh:.2} {tx:.2} {ty:.2} cm\n/{name} Do\nQ\n",
    )
}

/// SPEC: P4-EDIT-008 — apply a background as one undoable edit. The inverse is a
/// pre-write snapshot (`RestoreDocEdit`).
pub struct BackgroundEdit {
    pub pages: Vec<i32>,
    pub kind: BackgroundKind,
    pub opacity: f32,
}

impl<'a> Edit<PdfDocument<'a>> for BackgroundEdit {
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
        background_apply(doc, |bytes| add_background(bytes, &pages, &self.kind, self.opacity))
    }

    fn label(&self) -> &'static str {
        "background"
    }
}

/// Snapshot the live document, run a bytes → bytes edit, reload/replace it; the
/// inverse is the pre-edit snapshot. (Same shape as `watermark` / `image_edit`.)
fn background_apply<'a>(
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
