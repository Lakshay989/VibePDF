//! Bates numbering (P4.D5) — stamp a sequential legal-discovery identifier onto
//! every page: `{prefix}{zero-padded number}{suffix}` (e.g. `ABC000001`).
//!
//! SPEC: P4-EDIT-012. Mechanically this is [`crate::pdf::page_numbers`] with a
//! fixed format and a gap-free sequence: it reuses the same `cos` drawing
//! primitives (visual-space placement, exact base-14 metrics, the `/VibePDF`
//! marked-content tag, the snapshot → reload undo chassis) but stamps a distinct
//! computed id per page. Unlike page numbers it has **no exclusion** — every page
//! gets the next id, preserving the "unique, consecutive" invariant Bates exists
//! for. The spec's "across one or more PDFs" (cross-document batch) is deferred;
//! this stamps the open document (merge-then-Bates covers the multi-file case).

use std::fmt::Write as _;

use lopdf::{Document, Object, ObjectId};
use pdfium_render::prelude::PdfDocument;

use crate::error::CommandError;
use crate::pdf::cos::{
    append_page_content, base14_font_dict, base_font, ensure_winansi, escape_pdf_string,
    page_effective_box, page_rotation, parse_hex_color, register_page_resource, visual_cm_line,
    visual_transform, winansi_fits, wrap_decoration,
};
use crate::pdf::document::{pdfium, pdfium_lock};
use crate::pdf::font_metrics::text_width;
use crate::pdf::restore::RestoreDocEdit;
use crate::pdf::undo::Edit;

/// SPEC: P4-EDIT-012 — the Bates label for `value`: `prefix` + the number
/// zero-padded to at least `padding` digits + `suffix`. Padding is a *minimum*
/// width — a number wider than `padding` is never truncated (`(…,6,1_000_000)`
/// → `1000000`).
#[must_use]
pub fn bates_label(prefix: &str, suffix: &str, padding: usize, value: i64) -> String {
    format!("{prefix}{value:0>padding$}{suffix}")
}

/// SPEC: P4-EDIT-012 — stamp a Bates id in the `position` (`"header"` |
/// `"footer"`) margin, `align`ed (`"left"` | `"center"` | `"right"`), on every
/// page. The id on the page at 0-based index `i` is `start + i` (gap-free).
#[allow(clippy::too_many_arguments, clippy::cast_possible_wrap)]
pub fn add_bates(
    bytes: &[u8],
    position: &str,
    align: &str,
    prefix: &str,
    suffix: &str,
    padding: u32,
    start: i64,
    font_family: &str,
    size: f32,
    color: &str,
    margin: f32,
) -> Result<Vec<u8>, CommandError> {
    let header = match position {
        "header" => true,
        "footer" => false,
        other => return Err(CommandError::InvalidInput(format!("unknown position: {other}"))),
    };
    let alignment = Align::parse(align)?;
    if start < 0 {
        return Err(CommandError::InvalidInput("starting number must be zero or greater".into()));
    }
    // The digits are ASCII; only a caller-supplied prefix/suffix can escape WinAnsi.
    // Reject honestly (naming the offending chars) rather than mis-drawing them.
    if !winansi_fits(prefix) || !winansi_fits(suffix) {
        ensure_winansi(prefix)?;
        ensure_winansi(suffix)?;
    }
    let base = base_font(font_family, false, false)?;
    let rgb = parse_hex_color(color)?;
    let sz = size.max(1.0);
    let pad = padding as usize;

    let mut doc = Document::load_mem(bytes).map_err(cos_err)?;
    // `get_pages()` is keyed by 1-based page number, so iteration is in document
    // order; enumerate() gives the 0-based index the sequence counts from.
    let targets: Vec<(usize, ObjectId)> = doc.get_pages().into_values().enumerate().collect();

    for (idx, page_id) in targets {
        let shown = bates_label(prefix, suffix, pad, start + idx as i64);
        let rotate = page_rotation(&doc, page_id);
        let (vt, vw, vh) = visual_transform(rotate, page_effective_box(&doc, page_id));
        let font_name = register_bates_font(&mut doc, page_id, base)?;
        let y = if header { vh - margin - sz } else { margin };
        let w = text_width(base, &shown, sz);
        let x = match alignment {
            Align::Left => margin,
            Align::Center => (vw - w) / 2.0,
            Align::Right => vw - margin - w,
        };
        let content = bates_content(&font_name, rgb, sz, vt, x, y, &shown);
        append_page_content(&mut doc, page_id, wrap_decoration("bates", content))?;
    }

    let mut buf = Vec::new();
    doc.save_to(&mut buf)?;
    Ok(buf)
}

#[derive(Clone, Copy)]
enum Align {
    Left,
    Center,
    Right,
}

impl Align {
    fn parse(s: &str) -> Result<Self, CommandError> {
        Ok(match s {
            "left" => Self::Left,
            "center" => Self::Center,
            "right" => Self::Right,
            other => return Err(CommandError::InvalidInput(format!("unknown alignment: {other}"))),
        })
    }
}

/// Give `page_id` its own base-14 Bates font, returning the resource name.
fn register_bates_font(doc: &mut Document, page_id: ObjectId, base: &str) -> Result<String, CommandError> {
    let font = base14_font_dict(base);
    register_page_resource(doc, page_id, b"Font", "Fbn", Object::Dictionary(font))
}

/// The `q … Q` fragment drawing `shown` at visual origin `(x, y)`, mapped through
/// `vt` (page rotation + baseline). One fill colour + base-14 font.
#[allow(clippy::many_single_char_names)]
fn bates_content(
    font: &str,
    (r, g, b): (f32, f32, f32),
    size: f32,
    vt: [f32; 6],
    x: f32,
    y: f32,
    shown: &str,
) -> String {
    let mut content = String::new();
    let _ = writeln!(content, "q\n{}\n{r:.4} {g:.4} {b:.4} rg", visual_cm_line(vt));
    let _ = writeln!(
        content,
        "BT\n/{font} {size:.2} Tf\n{x:.2} {y:.2} Td\n({esc}) Tj\nET",
        esc = escape_pdf_string(shown),
    );
    content.push_str("Q\n");
    content
}

#[allow(clippy::needless_pass_by_value)]
fn cos_err(e: lopdf::Error) -> CommandError {
    CommandError::PdfError(format!("lopdf: {e}"))
}

/// SPEC: P4-EDIT-012 — apply Bates numbering as one undoable edit. The inverse is
/// a pre-write snapshot (`RestoreDocEdit`), same shape as `PageNumbersEdit`.
pub struct BatesEdit {
    pub position: String,
    pub align: String,
    pub prefix: String,
    pub suffix: String,
    pub padding: u32,
    pub start: i32,
    pub font_family: String,
    pub size: f32,
    pub color: String,
    pub margin: f32,
}

impl<'a> Edit<PdfDocument<'a>> for BatesEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        bates_apply(doc, |bytes| {
            add_bates(
                bytes,
                &self.position,
                &self.align,
                &self.prefix,
                &self.suffix,
                self.padding,
                i64::from(self.start),
                &self.font_family,
                self.size,
                &self.color,
                self.margin,
            )
        })
    }

    fn label(&self) -> &'static str {
        "bates"
    }
}

/// Snapshot the live document, run a bytes → bytes edit, reload/replace it; the
/// inverse is the pre-edit snapshot. (Same shape as page-numbers / header-footer.)
fn bates_apply<'a>(
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
    use super::bates_label;

    #[test]
    fn pads_to_min_width() {
        assert_eq!(bates_label("ABC", "", 6, 1), "ABC000001");
        assert_eq!(bates_label("ABC", "", 6, 50), "ABC000050");
        assert_eq!(bates_label("ABC", "", 6, 123_456), "ABC123456");
    }

    #[test]
    fn wider_than_padding_is_not_truncated() {
        assert_eq!(bates_label("ABC", "", 6, 1_000_000), "ABC1000000");
    }

    #[test]
    fn prefix_and_suffix_are_optional() {
        assert_eq!(bates_label("", "", 4, 7), "0007");
        assert_eq!(bates_label("", ".TIFF", 4, 7), "0007.TIFF");
        assert_eq!(bates_label("EX-", "-END", 3, 42), "EX-042-END");
    }

    #[test]
    fn padding_zero_or_one_is_natural_width() {
        assert_eq!(bates_label("A", "", 0, 5), "A5");
        assert_eq!(bates_label("A", "", 1, 5), "A5");
    }
}
