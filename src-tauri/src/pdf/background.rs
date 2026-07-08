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

use std::collections::HashSet;

use lopdf::{Dictionary, Document, Object, ObjectId, Stream};
use pdfium_render::prelude::PdfDocument;

use crate::error::CommandError;
use crate::pdf::cos::{
    page_effective_box, page_media_box, page_rotation, parse_hex_color, prepend_page_content,
    register_page_resource, visual_cm_line, visual_transform, wrap_decoration,
};
use crate::pdf::document::{pdfium, pdfium_lock};
use crate::pdf::image_xobject::embed_image;
use crate::pdf::restore::RestoreDocEdit;
use crate::pdf::undo::Edit;

/// What to paint behind the page: a solid colour, a raster image, or a page from
/// another PDF.
pub enum BackgroundKind {
    /// `#rrggbb`.
    Color(String),
    /// Raw PNG/JPEG bytes (read by the command; the actor stays byte-pure).
    Image(Vec<u8>),
    /// A page from another PDF (raw source bytes + 0-based page index). Imported
    /// as a Form `XObject` and drawn contain-fit.
    PdfPage { source: Vec<u8>, page: usize },
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
    // an image / import a source page once and reference it from every page.
    let rgb = match kind {
        BackgroundKind::Color(hex) => Some(parse_hex_color(hex)?),
        _ => None,
    };
    let image = match kind {
        BackgroundKind::Image(data) => Some(embed_image(&mut doc, data)?),
        _ => None,
    };
    // (form_id, source MediaBox) — the imported page, shared across target pages.
    let pdf_form = match kind {
        BackgroundKind::PdfPage { source, page } => Some(import_page_as_form(&mut doc, source, *page)?),
        _ => None,
    };

    for page_id in targets {
        // A colour fill covers the full MediaBox (bleed-safe); image / PDF-page
        // placement targets the VISUAL box (displayed CropBox, after /Rotate)
        // so it reads upright and covers what the user actually sees.
        let [x0, y0, x1, y1] = page_media_box(&doc, page_id);
        let (w, h) = (x1 - x0, y1 - y0);
        let rotate = page_rotation(&doc, page_id);
        let (vt, vw, vh) = visual_transform(rotate, page_effective_box(&doc, page_id));

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
                image_content(&gs_name, &name, vt, [vw, vh], img.width, img.height)
            }
            BackgroundKind::PdfPage { .. } => {
                let (form_id, src_bbox) =
                    pdf_form.ok_or_else(|| CommandError::Internal("source page not imported".into()))?;
                let name = register_page_resource(
                    &mut doc,
                    page_id,
                    b"XObject",
                    "Bgpdf",
                    Object::Reference(form_id),
                )?;
                pdf_content(&gs_name, &name, vt, src_bbox, [vw, vh])
            }
        };

        // A background always draws behind the page's own content.
        prepend_page_content(&mut doc, page_id, wrap_decoration("background", content))?;
    }

    let mut buf = Vec::new();
    doc.save_to(&mut buf)?;
    Ok(buf)
}

#[allow(clippy::needless_pass_by_value)]
fn cos_err(e: lopdf::Error) -> CommandError {
    CommandError::PdfError(format!("lopdf: {e}"))
}

/// SPEC: P4-EDIT-008 (P4.D1b) — import the 0-based `page` of `source_bytes` into
/// `dest` as a Form `XObject`, returning its id + source `MediaBox`. The page's
/// content becomes the form's stream; its **effective `/Resources`** (resolved up
/// the `/Parent` chain) become the form's, and the transitive object closure of
/// those resources is copied in — only that subtree, not the whole source doc.
/// Source ids are renumbered above `dest`'s first, so nothing collides.
///
/// Limitation: the source page's `/Rotate` is ignored (Form `XObject`s don't
/// carry page rotation) — a rotated source imports unrotated.
fn import_page_as_form(
    dest: &mut Document,
    source_bytes: &[u8],
    page: usize,
) -> Result<(ObjectId, [f32; 4]), CommandError> {
    let mut src = Document::load_mem(source_bytes).map_err(cos_err)?;
    // Shift every source id above the dest's so a copy can't collide.
    src.renumber_objects_with(dest.max_id + 1);

    let page_no = u32::try_from(page)
        .ok()
        .map(|n| n + 1)
        .ok_or_else(|| CommandError::InvalidInput(format!("bad source page: {page}")))?;
    let page_id = *src
        .get_pages()
        .get(&page_no)
        .ok_or_else(|| CommandError::InvalidInput(format!("source page out of range: {page}")))?;

    let bbox = page_media_box(&src, page_id);
    let content = src.get_page_content(page_id).map_err(cos_err)?;
    let resources = effective_resources(&src, page_id);

    // Copy only the objects the page's resources transitively reference.
    let mut closure = HashSet::new();
    collect_refs(&src, &Object::Dictionary(resources.clone()), &mut closure);
    for id in &closure {
        if let Some(obj) = src.objects.get(id) {
            dest.objects.insert(*id, obj.clone());
        }
    }
    // Adopt the source's id ceiling so the new Form gets a fresh, non-colliding id.
    dest.max_id = dest.max_id.max(src.max_id);

    let mut form = Dictionary::new();
    form.set("Type", Object::Name(b"XObject".to_vec()));
    form.set("Subtype", Object::Name(b"Form".to_vec()));
    form.set("FormType", Object::Integer(1));
    form.set(
        "BBox",
        Object::Array(bbox.iter().map(|&v| Object::Real(v)).collect()),
    );
    form.set("Resources", Object::Dictionary(resources));
    let form_id = dest.add_object(Stream::new(form, content));
    Ok((form_id, bbox))
}

/// A page's effective `/Resources` as an owned dict, resolving a reference and
/// walking the `/Parent` chain (resources can be inherited). Empty when absent.
fn effective_resources(doc: &Document, page_id: ObjectId) -> Dictionary {
    let mut cur = Some(page_id);
    while let Some(id) = cur {
        let Ok(dict) = doc.get_dictionary(id) else { break };
        match dict.get(b"Resources") {
            Ok(Object::Dictionary(d)) => return d.clone(),
            Ok(Object::Reference(rid)) => {
                if let Ok(d) = doc.get_dictionary(*rid) {
                    return d.clone();
                }
            }
            _ => {}
        }
        cur = dict.get(b"Parent").and_then(Object::as_reference).ok();
    }
    Dictionary::new()
}

/// Collect every object id transitively referenced from `obj`.
fn collect_refs(doc: &Document, obj: &Object, acc: &mut HashSet<ObjectId>) {
    match obj {
        Object::Reference(id) if acc.insert(*id) => {
            if let Ok(o) = doc.get_object(*id) {
                collect_refs(doc, o, acc);
            }
        }
        Object::Array(a) => a.iter().for_each(|o| collect_refs(doc, o, acc)),
        Object::Dictionary(d) => d.iter().for_each(|(_, o)| collect_refs(doc, o, acc)),
        Object::Stream(s) => s.dict.iter().for_each(|(_, o)| collect_refs(doc, o, acc)),
        _ => {}
    }
}

/// `q … Q`: map visual → page space (`vt`), then paint the imported page Form
/// (drawn in its `src_bbox` space) contain-fit and centred inside the visual
/// box `[tw, th]` (the whole source page stays visible — never cropped).
#[allow(clippy::many_single_char_names)]
fn pdf_content(gs: &str, name: &str, vt: [f32; 6], [sx0, sy0, sx1, sy1]: [f32; 4], [tw, th]: [f32; 2]) -> String {
    let (sw, sh) = (sx1 - sx0, sy1 - sy0);
    let scale = if sw > 0.0 && sh > 0.0 { (tw / sw).min(th / sh) } else { 1.0 };
    let (pw, ph) = (sw * scale, sh * scale);
    // Map a BBox point (x, y) → scale·(x − sx0) + ox, scale·(y − sy0) + oy.
    let e = (tw - pw) / 2.0 - scale * sx0;
    let f = (th - ph) / 2.0 - scale * sy0;
    format!(
        "q\n/{gs} gs\n{vtl}\n{scale:.5} 0 0 {scale:.5} {e:.2} {f:.2} cm\n/{name} Do\nQ\n",
        vtl = visual_cm_line(vt),
    )
}

/// `q … Q`: fill the page rect `[x0, y0, w, h]` with `(r, g, b)`.
#[allow(clippy::many_single_char_names)]
fn color_content(gs: &str, (r, g, b): (f32, f32, f32), [x0, y0, w, h]: [f32; 4]) -> String {
    format!("q\n/{gs} gs\n{r:.4} {g:.4} {b:.4} rg\n{x0:.2} {y0:.2} {w:.2} {h:.2} re\nf\nQ\n")
}

/// `q … Q`: map visual → page space (`vt`), clip to the visual box, then paint
/// the image cover-fit (fills the visible page, preserves aspect, crops
/// overflow). The image draws in the unit square, so the second `cm` scales it
/// to `sw`×`sh` and centres it in visual coordinates.
#[allow(clippy::cast_precision_loss, clippy::many_single_char_names)]
fn image_content(gs: &str, name: &str, vt: [f32; 6], [w, h]: [f32; 2], iw: u32, ih: u32) -> String {
    let cover = (w / iw as f32).max(h / ih as f32);
    let (sw, sh) = (iw as f32 * cover, ih as f32 * cover);
    let (tx, ty) = ((w - sw) / 2.0, (h - sh) / 2.0);
    format!(
        "q\n/{gs} gs\n{vtl}\n0.00 0.00 {w:.2} {h:.2} re\nW n\n\
         {sw:.2} 0 0 {sh:.2} {tx:.2} {ty:.2} cm\n/{name} Do\nQ\n",
        vtl = visual_cm_line(vt),
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
