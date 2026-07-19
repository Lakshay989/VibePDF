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

use std::collections::HashMap;

use lopdf::{Document, Object, ObjectId};
use pdfium_render::prelude::PdfDocument;

use crate::error::CommandError;
use crate::pdf::cos::{
    append_page_content, base_font, escape_pdf_string, page_effective_box, page_rotation,
    parse_hex_color, register_page_resource, visual_cm_line, visual_transform, wrap_decoration,
};
use crate::pdf::document::{pdfium, pdfium_lock};
use crate::pdf::font_embed_cid::{build_cid_font, place_cid_run, CidRun};
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

    // FABLE_REVIEW 3.2 stage-2 (P4.HF5): if any text needs glyphs the base-14
    // fonts lack ({n}/{total} substitute to ASCII digits, so the templates + date
    // decide), embed a covering system font through `PDFium` instead of the base-14
    // lopdf path. WinAnsi text keeps the cheap, unchanged path below.
    if [left, center, right, date].iter().any(|s| !crate::pdf::cos::winansi_fits(s)) {
        let combined = format!("{left}{center}{right}{date}");
        let Some(font_bytes) = crate::pdf::font_resolver::covering_font_bytes(&combined) else {
            // No embeddable font on this machine — fall back to the honest HF3
            // rejection, which names the offending characters. (The trailing
            // return is unreachable: `combined` is non-WinAnsi here, so
            // `ensure_winansi` always errors.)
            crate::pdf::cos::ensure_winansi(&combined)?;
            return Err(CommandError::Internal(
                "non-WinAnsi header/footer text unexpectedly passed the WinAnsi check".into(),
            ));
        };
        return add_header_footer_embedded(
            bytes, pages, header, left, center, right, rgb, sz, margin, date, &font_bytes,
        );
    }

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
        // Lay out in the page's VISUAL space (the displayed CropBox, after
        // /Rotate), so the footer lands at the visible bottom even on rotated
        // or cropped pages; `vt` maps visual coords back into page space.
        let rotate = page_rotation(&doc, page_id);
        let (vt, vw, vh) = visual_transform(rotate, page_effective_box(&doc, page_id));
        let font_name = register_hf_font(&mut doc, page_id, base)?;
        let y = if header { vh - margin - sz } else { margin };
        let content = header_footer_content(
            &font_name,
            rgb,
            sz,
            base,
            vt,
            [0.0, vw],
            y,
            margin,
            &[(left, Align::Left), (center, Align::Center), (right, Align::Right)],
            page_no,
            total,
            date,
        );
        // A header/footer draws on top of the page's own content.
        append_page_content(&mut doc, page_id, wrap_decoration("header-footer", content))?;
    }

    let mut buf = Vec::new();
    doc.save_to(&mut buf)?;
    Ok(buf)
}

/// The non-WinAnsi header/footer path (`FABLE_REVIEW` 3.2 stage-2 / CID-path
/// unification): embed a hand-built CID font (`build_cid_font`) and place each
/// L/C/R segment as marked-content-wrapped CID page content via [`place_cid_run`].
/// This shares the `/AP` writers' backend, so the embedded path gains **exact
/// metrics** (`cid.width` — the `FABLE_REVIEW` §3.10 embed-path fix) and the **HF2
/// `/VibePDF` splice tag** (an embedded header/footer is now operator-removable),
/// with no `PDFium` round-trip. Placement uses the same visual-space transform as
/// the base-14 path, so rotated/cropped pages still land upright.
#[allow(clippy::too_many_arguments)]
fn add_header_footer_embedded(
    bytes: &[u8],
    pages: &[usize],
    header: bool,
    left: &str,
    center: &str,
    right: &str,
    rgb: (f32, f32, f32),
    sz: f32,
    margin: f32,
    date: &str,
    font_bytes: &[u8],
) -> Result<Vec<u8>, CommandError> {
    // One resolved L/C/R segment awaiting placement (pass 1 → pass 2).
    struct Seg {
        page_id: ObjectId,
        vt: [f32; 6],
        vw: f32,
        vy: f32,
        align: Align,
        shown: String,
    }

    let mut doc = Document::load_mem(bytes).map_err(cos_err)?;
    let total = doc.get_pages().len();
    let page_map = doc.get_pages();

    // Pass 1: resolve each page's geometry + its substituted L/C/R segments,
    // accumulating every character so the CID subset covers them all.
    let mut segs: Vec<Seg> = Vec::new();
    let mut all_text = String::new();
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
        let vy = if header { vh - margin - sz } else { margin };
        for (template, align) in
            [(left, Align::Left), (center, Align::Center), (right, Align::Right)]
        {
            if template.trim().is_empty() {
                continue;
            }
            let shown = substitute(template, p + 1, total, date);
            all_text.push_str(&shown);
            segs.push(Seg { page_id, vt, vw, vy, align, shown });
        }
    }

    // Pass 2: build the CID font once, then place each segment at its exact
    // width. The font is registered once per page (its dict is shared).
    if !segs.is_empty() {
        let cid = build_cid_font(&mut doc, font_bytes, &all_text)?;
        let mut font_by_page: HashMap<ObjectId, String> = HashMap::new();
        for seg in &segs {
            let font_name = if let Some(name) = font_by_page.get(&seg.page_id) { name.clone() } else {
                let name = register_page_resource(
                    &mut doc,
                    seg.page_id,
                    b"Font",
                    "Fcid",
                    Object::Dictionary(cid.font_dict.clone()),
                )?;
                font_by_page.insert(seg.page_id, name.clone());
                name
            };
            let vx = aligned_x(seg.align, seg.vw, margin, cid.width(&seg.shown, sz));
            place_cid_run(
                &mut doc,
                seg.page_id,
                &font_name,
                &cid,
                &CidRun {
                    text: &seg.shown,
                    size: sz,
                    color: rgb,
                    // Bake the aligned origin (vx, vy) through the visual transform,
                    // so the run's matrix carries both page rotation and baseline.
                    matrix: place_in_visual_space(seg.vt, vx, seg.vy),
                    opacity: 1.0,
                    behind: false,
                    underline: None,
                    kind: "header-footer",
                },
            )?;
        }
    }

    let mut buf = Vec::new();
    doc.save_to(&mut buf)?;
    Ok(buf)
}

/// The left edge for `align` within a `[0, vw]` visual box, `margin` inset.
fn aligned_x(align: Align, vw: f32, margin: f32, width: f32) -> f32 {
    match align {
        Align::Left => margin,
        Align::Center => (vw - width) / 2.0,
        Align::Right => vw - margin - width,
    }
}

/// Map a visual-space origin `(vx, vy)` through `vt = [a b c d e f]` into the
/// page-space text matrix `[a b c d e' f']`.
#[allow(clippy::many_single_char_names)]
fn place_in_visual_space([a, b, c, d, e, f]: [f32; 6], vx: f32, vy: f32) -> [f32; 6] {
    [a, b, c, d, a * vx + c * vy + e, b * vx + d * vy + f]
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
    let font = crate::pdf::cos::base14_font_dict(base);
    crate::pdf::cos::register_page_resource(doc, page_id, b"Font", "Fhf", Object::Dictionary(font))
}

/// Build the `q … Q` fragment drawing each non-empty position at its aligned `x`
/// and the shared baseline `y`, in visual coordinates mapped through `vt`. All
/// three positions share one fill colour + font.
#[allow(clippy::too_many_arguments, clippy::many_single_char_names, clippy::cast_precision_loss)]
fn header_footer_content(
    font: &str,
    (r, g, b): (f32, f32, f32),
    size: f32,
    base: &str,
    vt: [f32; 6],
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
    let _ = writeln!(content, "q\n{}\n{r:.4} {g:.4} {b:.4} rg", visual_cm_line(vt));
    for (template, align) in parts {
        if template.trim().is_empty() {
            continue;
        }
        let shown = substitute(template, page_no, total, date);
        // Exact base-14 advance width so centre/right land where the viewer
        // renders them (FABLE_REVIEW §3.10 / P4.HF16).
        let w = crate::pdf::font_metrics::text_width(base, &shown, size);
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use pdfium_render::prelude::*;

    use super::add_header_footer_embedded;
    use crate::pdf::document::{pdfium, pdfium_lock};

    fn hello() -> Vec<u8> {
        std::fs::read("../tests/fixtures/basic/hello.pdf").expect("hello.pdf")
    }

    fn coptic_font() -> Vec<u8> {
        std::fs::read("../tests/fixtures/fonts/NotoSansCoptic-Regular.ttf")
            .expect("committed Coptic fixture font")
    }

    /// Concatenate every text object's text on page 0 (proves the `/ToUnicode`
    /// map round-trips through save/reopen).
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

    /// P4.HF5 / CID-path unification: a non-WinAnsi footer takes the embedded
    /// path and the Unicode survives reopen, alongside the page's own (base-14)
    /// text. Deterministic — drives the private embedded path with the committed
    /// Coptic fixture font.
    #[test]
    fn embedded_footer_renders_and_extracts_unicode() {
        let coptic = "\u{2C81}\u{2C83}\u{2C85}"; // Ⲁ Ⲃ Ⲅ
        let out = add_header_footer_embedded(
            &hello(),
            &[0],
            false, // footer
            "",
            coptic,
            "",
            (0.0, 0.0, 0.0),
            24.0,
            36.0,
            "d",
            &coptic_font(),
        )
        .expect("embed footer");
        let text = page0_text(&out);
        assert!(text.contains(coptic), "embedded Coptic footer round-trips; got {text:?}");
        assert!(text.contains("VibePDF"), "the page's own base-14 text is intact");
    }

    /// CID-path unification (phase 1): the embedded footer is now hand-built CID
    /// page content — a hex `Tj`, not a `PDFium` object — wrapped in the HF2
    /// `/VibePDF` marked-content tag, so it is operator-splice-removable.
    #[test]
    fn embedded_footer_is_cid_and_tagged() {
        use lopdf::{Document, Object, StringFormat};
        let out = add_header_footer_embedded(
            &hello(),
            &[0],
            false,
            "",
            "\u{2C81}\u{2C83}", // Ⲁ Ⲃ
            "",
            (0.0, 0.0, 0.0),
            12.0,
            36.0,
            "d",
            &coptic_font(),
        )
        .expect("embed footer");

        let doc = Document::load_mem(&out).expect("reopen");
        let page_id = *doc.get_pages().get(&1).expect("page 1");
        let content = doc.get_and_decode_page_content(page_id).expect("decode content");

        assert!(
            content.operations.iter().any(|op| op.operator == "BDC"
                && matches!(op.operands.first(), Some(Object::Name(n)) if n == b"VibePDF")),
            "embedded footer must carry the /VibePDF marked-content tag (HF2)"
        );
        assert!(
            content.operations.iter().any(|op| op.operator == "Tj"
                && matches!(
                    op.operands.first(),
                    Some(Object::String(_, StringFormat::Hexadecimal))
                )),
            "must draw text as a CID (hex) Tj, not a PDFium text object"
        );
        assert!(
            content.operations.iter().any(|op| op.operator == "cm"),
            "the run is placed through a cm matrix"
        );
    }
}
