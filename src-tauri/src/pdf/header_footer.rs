//! Header / footer (P4.D3) — draw text in the top or bottom margin of selected
//! pages, with left / centre / right positions and per-page placeholders.
//!
//! SPEC: P4-EDIT-010. A header/footer is **page content on top** (it overlays),
//! so it's `append_page_content` of a `q … Q` fragment — the watermark's text
//! path, positioned in the margin instead of rotated-centred. Each of the three
//! positions carries a template whose `{n}` / `{total}` / `{date}` placeholders
//! are substituted per page; `{date}` is supplied by the caller (the frontend's
//! locale-formatted today) so no date dependency is needed. Bytes → bytes; the
//! actor wraps it in the snapshot → reload chassis ([`header_footer_apply`]),
//! inverse `RestoreDocEdit`.

use lopdf::{Dictionary, Document, Object, ObjectId};
use pdfium_render::prelude::PdfDocument;

use crate::error::CommandError;
use crate::pdf::cos::{
    append_page_content, base_font, escape_pdf_string, font_avg_em, page_media_box, parse_hex_color,
};
use crate::pdf::document::{pdfium, pdfium_lock};
use crate::pdf::restore::RestoreDocEdit;
use crate::pdf::undo::Edit;

/// Substitute the header/footer placeholders in `template`: `{n}` → the 1-based
/// `page_no`, `{total}` → the document page count, `{date}` → `date` (verbatim).
#[must_use]
pub fn substitute(template: &str, page_no: usize, total: usize, date: &str) -> String {
    template
        .replace("{n}", &page_no.to_string())
        .replace("{total}", &total.to_string())
        .replace("{date}", date)
}

/// SPEC: P4-EDIT-010 — draw `left` / `center` / `right` text (any empty ones
/// skipped) in the `position` (`"header"` | `"footer"`) margin of each 0-based
/// page in `pages`, in `font_family`/`size`/`color`, `margin` points from the
/// edge. Placeholders are substituted per page; `date` is the `{date}` value.
#[allow(clippy::too_many_arguments)]
pub fn add_header_footer(
    bytes: &[u8],
    pages: &[usize],
    position: &str,
    left: &str,
    center: &str,
    right: &str,
    font_family: &str,
    size: f32,
    color: &str,
    margin: f32,
    date: &str,
) -> Result<Vec<u8>, CommandError> {
    if pages.is_empty() {
        return Err(CommandError::InvalidInput("no pages selected for header/footer".into()));
    }
    let header = match position {
        "header" => true,
        "footer" => false,
        other => return Err(CommandError::InvalidInput(format!("unknown position: {other}"))),
    };
    if left.trim().is_empty() && center.trim().is_empty() && right.trim().is_empty() {
        return Err(CommandError::InvalidInput("header/footer text is empty".into()));
    }
    let base = base_font(font_family, false, false)?;
    let rgb = parse_hex_color(color)?;
    let sz = size.max(1.0);

    let mut doc = Document::load_mem(bytes).map_err(cos_err)?;
    let total = doc.get_pages().len();

    // Resolve + range-check targets (keeping each page's 1-based number for `{n}`).
    let page_map = doc.get_pages();
    let mut targets: Vec<(usize, ObjectId)> = Vec::with_capacity(pages.len());
    for &p in pages {
        let page_no = u32::try_from(p)
            .ok()
            .map(|n| n + 1)
            .ok_or_else(|| CommandError::InvalidInput(format!("bad page index: {p}")))?;
        let id = *page_map
            .get(&page_no)
            .ok_or_else(|| CommandError::InvalidInput(format!("page index out of range: {p}")))?;
        targets.push((p + 1, id));
    }

    for (page_no, page_id) in targets {
        let [x0, y0, x1, y1] = page_media_box(&doc, page_id);
        let font_name = register_hf_font(&mut doc, page_id, base)?;
        let y = if header { y1 - margin - sz } else { y0 + margin };
        let content = header_footer_content(
            &font_name,
            rgb,
            sz,
            base,
            [x0, x1],
            y,
            margin,
            &[(left, Align::Left), (center, Align::Center), (right, Align::Right)],
            page_no,
            total,
            date,
        );
        // A header/footer draws on top of the page's own content.
        append_page_content(&mut doc, page_id, content)?;
    }

    let mut buf = Vec::new();
    doc.save_to(&mut buf)?;
    Ok(buf)
}

#[allow(clippy::needless_pass_by_value)]
fn cos_err(e: lopdf::Error) -> CommandError {
    CommandError::PdfError(format!("lopdf: {e}"))
}

#[derive(Clone, Copy)]
enum Align {
    Left,
    Center,
    Right,
}

/// Give `page_id` its own base-14 header/footer font, returning the resource name.
fn register_hf_font(doc: &mut Document, page_id: ObjectId, base: &str) -> Result<String, CommandError> {
    let mut font = Dictionary::new();
    font.set("Type", Object::Name(b"Font".to_vec()));
    font.set("Subtype", Object::Name(b"Type1".to_vec()));
    font.set("BaseFont", Object::Name(base.as_bytes().to_vec()));
    crate::pdf::cos::register_page_resource(doc, page_id, b"Font", "Fhf", Object::Dictionary(font))
}

/// Build the `q … Q` fragment drawing each non-empty position at its aligned `x`
/// and the shared baseline `y`. All three share one fill colour + font.
#[allow(clippy::too_many_arguments, clippy::many_single_char_names, clippy::cast_precision_loss)]
fn header_footer_content(
    font: &str,
    (r, g, b): (f32, f32, f32),
    size: f32,
    base: &str,
    [x0, x1]: [f32; 2],
    y: f32,
    margin: f32,
    parts: &[(&str, Align)],
    page_no: usize,
    total: usize,
    date: &str,
) -> String {
    use std::fmt::Write as _;
    let mut content = String::new();
    let _ = writeln!(content, "q\n{r:.4} {g:.4} {b:.4} rg");
    for (template, align) in parts {
        if template.trim().is_empty() {
            continue;
        }
        let shown = substitute(template, page_no, total, date);
        let w = size * font_avg_em(base) * shown.chars().count() as f32;
        let x = match align {
            Align::Left => x0 + margin,
            Align::Center => x0 + (x1 - x0 - w) / 2.0,
            Align::Right => x1 - margin - w,
        };
        let _ = writeln!(
            content,
            "BT\n/{font} {size:.2} Tf\n{x:.2} {y:.2} Td\n({esc}) Tj\nET",
            esc = escape_pdf_string(&shown),
        );
    }
    content.push_str("Q\n");
    content
}

/// SPEC: P4-EDIT-010 — apply a header/footer as one undoable edit. The inverse is
/// a pre-write snapshot (`RestoreDocEdit`).
pub struct HeaderFooterEdit {
    pub pages: Vec<i32>,
    pub position: String,
    pub left: String,
    pub center: String,
    pub right: String,
    pub font_family: String,
    pub size: f32,
    pub color: String,
    pub margin: f32,
    pub date: String,
}

impl<'a> Edit<PdfDocument<'a>> for HeaderFooterEdit {
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
        header_footer_apply(doc, |bytes| {
            add_header_footer(
                bytes,
                &pages,
                &self.position,
                &self.left,
                &self.center,
                &self.right,
                &self.font_family,
                self.size,
                &self.color,
                self.margin,
                &self.date,
            )
        })
    }

    fn label(&self) -> &'static str {
        "header-footer"
    }
}

/// Snapshot the live document, run a bytes → bytes edit, reload/replace it; the
/// inverse is the pre-edit snapshot. (Same shape as `watermark` / `background`.)
fn header_footer_apply<'a>(
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
