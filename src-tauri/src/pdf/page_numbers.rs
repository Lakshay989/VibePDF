//! Page numbers (P4.D4) — stamp a page number onto each page's header or footer
//! margin, in a chosen format, with a starting number and per-page exclusions.
//!
//! SPEC: P4-EDIT-011. A page number is page content on top (an overlay), like the
//! header/footer (P4.D3, [`crate::pdf::header_footer`]) it is modelled on:
//! `append_page_content` of a `q … Q` text fragment positioned in the margin.
//! Two things differ from header/footer: it draws **one computed value per page**
//! (from the format + starting number, not a fixed template), and every format is
//! **pure ASCII** — so it never needs the CID / non-WinAnsi embedding path that
//! header/footer carries. Bytes → bytes; the actor wraps it in the snapshot →
//! reload chassis ([`page_numbers_apply`]), inverse `RestoreDocEdit`.

use std::collections::HashSet;
use std::fmt::Write as _;

use lopdf::{Document, Object, ObjectId};
use pdfium_render::prelude::PdfDocument;

use crate::error::CommandError;
use crate::pdf::cos::{
    append_page_content, base14_font_dict, base_font, escape_pdf_string, page_effective_box,
    page_rotation, parse_hex_color, register_page_resource, visual_cm_line, visual_transform,
    wrap_decoration,
};
use crate::pdf::document::{pdfium, pdfium_lock};
use crate::pdf::font_metrics::text_width;
use crate::pdf::restore::RestoreDocEdit;
use crate::pdf::undo::Edit;

/// The number-rendering styles from SPEC P4-EDIT-011: `1`, `1/N`, `Page 1 of N`,
/// lower/upper Roman (`i`/`I`), lower/upper alphabetic (`a`/`A`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NumberFormat {
    /// `1`
    Decimal,
    /// `1/N`
    DecimalSlashTotal,
    /// `Page 1 of N`
    PageXofN,
    /// `i`, `ii`, `iii`, …
    LowerRoman,
    /// `I`, `II`, `III`, …
    UpperRoman,
    /// `a`, `b`, …, `z`, `aa`, …
    LowerAlpha,
    /// `A`, `B`, …, `Z`, `AA`, …
    UpperAlpha,
}

impl NumberFormat {
    /// Parse the wire string (matching the frontend union) into a format.
    pub fn parse(s: &str) -> Result<Self, CommandError> {
        Ok(match s {
            "decimal" => Self::Decimal,
            "decimal-slash-total" => Self::DecimalSlashTotal,
            "page-x-of-n" => Self::PageXofN,
            "lower-roman" => Self::LowerRoman,
            "upper-roman" => Self::UpperRoman,
            "lower-alpha" => Self::LowerAlpha,
            "upper-alpha" => Self::UpperAlpha,
            other => return Err(CommandError::InvalidInput(format!("unknown page-number format: {other}"))),
        })
    }
}

/// SPEC: P4-EDIT-011 — render `value` (this page's number) in `format`. `total` is
/// only consulted by the `1/N` / `Page 1 of N` composites. Roman and alphabetic
/// numerals are undefined for non-positive values, so those error; the caller
/// enforces `start >= 1`, which keeps every `value` positive.
pub fn format_number(format: NumberFormat, value: i64, total: i64) -> Result<String, CommandError> {
    Ok(match format {
        NumberFormat::Decimal => value.to_string(),
        NumberFormat::DecimalSlashTotal => format!("{value}/{total}"),
        NumberFormat::PageXofN => format!("Page {value} of {total}"),
        NumberFormat::LowerRoman => to_roman(value)?.to_lowercase(),
        NumberFormat::UpperRoman => to_roman(value)?,
        NumberFormat::LowerAlpha => to_alpha(value)?,
        NumberFormat::UpperAlpha => to_alpha(value)?.to_uppercase(),
    })
}

/// Uppercase Roman numeral for `n >= 1` (subtractive; `M` repeats past 3999).
fn to_roman(n: i64) -> Result<String, CommandError> {
    const UNITS: [(i64, &str); 13] = [
        (1000, "M"), (900, "CM"), (500, "D"), (400, "CD"), (100, "C"), (90, "XC"),
        (50, "L"), (40, "XL"), (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I"),
    ];
    if n < 1 {
        return Err(CommandError::InvalidInput(format!(
            "Roman numerals need a positive number, got {n}"
        )));
    }
    let mut n = n;
    let mut out = String::new();
    for (val, sym) in UNITS {
        while n >= val {
            out.push_str(sym);
            n -= val;
        }
    }
    Ok(out)
}

/// Lowercase bijective base-26 (spreadsheet-style): 1→`a`, 26→`z`, 27→`aa`, …
fn to_alpha(n: i64) -> Result<String, CommandError> {
    if n < 1 {
        return Err(CommandError::InvalidInput(format!(
            "Alphabetic numbering needs a positive number, got {n}"
        )));
    }
    let mut n = n;
    let mut rev = Vec::new();
    while n > 0 {
        let rem = (n - 1) % 26;
        rev.push(b'a' + u8::try_from(rem).unwrap_or(0));
        n = (n - 1) / 26;
    }
    rev.reverse();
    Ok(String::from_utf8(rev).unwrap_or_default())
}

/// SPEC: P4-EDIT-011 — stamp a page number in the `position` (`"header"` |
/// `"footer"`) margin, `align`ed (`"left"` | `"center"` | `"right"`), on every
/// page except the 0-based indices in `exclude`. The number shown on the page at
/// 0-based index `i` is `start + i` (exclusions suppress the stamp but do **not**
/// shift the sequence); `Page 1 of N`/`1/N` use `total = start + page_count - 1`.
#[allow(clippy::too_many_arguments, clippy::cast_possible_wrap)]
pub fn add_page_numbers(
    bytes: &[u8],
    exclude: &[usize],
    position: &str,
    align: &str,
    format: &str,
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
    let format = NumberFormat::parse(format)?;
    if start < 1 {
        return Err(CommandError::InvalidInput("starting number must be at least 1".into()));
    }
    let base = base_font(font_family, false, false)?;
    let rgb = parse_hex_color(color)?;
    let sz = size.max(1.0);

    let mut doc = Document::load_mem(bytes).map_err(cos_err)?;
    // `get_pages()` is a BTreeMap keyed by 1-based page number, so iteration is in
    // document order; enumerate() gives the 0-based index the caller speaks in.
    let targets: Vec<(usize, ObjectId)> =
        doc.get_pages().into_values().enumerate().collect();
    let page_count = targets.len() as i64;
    let total = start + page_count - 1;
    let exclude_set: HashSet<usize> = exclude.iter().copied().collect();

    for (idx, page_id) in targets {
        if exclude_set.contains(&idx) {
            continue;
        }
        let shown = format_number(format, start + idx as i64, total)?;
        // Lay out in VISUAL space (displayed CropBox after /Rotate) so the number
        // lands in the visible margin even on rotated/cropped pages.
        let rotate = page_rotation(&doc, page_id);
        let (vt, vw, vh) = visual_transform(rotate, page_effective_box(&doc, page_id));
        let font_name = register_pn_font(&mut doc, page_id, base)?;
        let y = if header { vh - margin - sz } else { margin };
        let w = text_width(base, &shown, sz);
        let x = match alignment {
            Align::Left => margin,
            Align::Center => (vw - w) / 2.0,
            Align::Right => vw - margin - w,
        };
        let content = page_number_content(&font_name, rgb, sz, vt, x, y, &shown);
        append_page_content(&mut doc, page_id, wrap_decoration("page-number", content))?;
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

/// Give `page_id` its own base-14 page-number font, returning the resource name.
fn register_pn_font(doc: &mut Document, page_id: ObjectId, base: &str) -> Result<String, CommandError> {
    let font = base14_font_dict(base);
    register_page_resource(doc, page_id, b"Font", "Fpn", Object::Dictionary(font))
}

/// The `q … Q` fragment drawing `shown` at visual origin `(x, y)`, mapped through
/// `vt` (page rotation + baseline). One fill colour + base-14 font.
#[allow(clippy::many_single_char_names)]
fn page_number_content(
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

/// SPEC: P4-EDIT-011 — apply page numbers as one undoable edit. The inverse is a
/// pre-write snapshot (`RestoreDocEdit`), same shape as `HeaderFooterEdit`.
pub struct PageNumbersEdit {
    pub exclude: Vec<i32>,
    pub position: String,
    pub align: String,
    pub format: String,
    pub start: i32,
    pub font_family: String,
    pub size: f32,
    pub color: String,
    pub margin: f32,
}

impl<'a> Edit<PdfDocument<'a>> for PageNumbersEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        let exclude: Vec<usize> = self
            .exclude
            .iter()
            .map(|&p| {
                usize::try_from(p)
                    .map_err(|_| CommandError::InvalidInput(format!("negative page index: {p}")))
            })
            .collect::<Result<_, _>>()?;
        page_numbers_apply(doc, |bytes| {
            add_page_numbers(
                bytes,
                &exclude,
                &self.position,
                &self.align,
                &self.format,
                i64::from(self.start),
                &self.font_family,
                self.size,
                &self.color,
                self.margin,
            )
        })
    }

    fn label(&self) -> &'static str {
        "page-numbers"
    }
}

/// Snapshot the live document, run a bytes → bytes edit, reload/replace it; the
/// inverse is the pre-edit snapshot. (Same shape as header/footer / watermark.)
fn page_numbers_apply<'a>(
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
    use super::{format_number, NumberFormat};

    #[test]
    fn decimal_and_composites() {
        assert_eq!(format_number(NumberFormat::Decimal, 1, 10).unwrap(), "1");
        assert_eq!(format_number(NumberFormat::Decimal, 7, 10).unwrap(), "7");
        assert_eq!(format_number(NumberFormat::DecimalSlashTotal, 3, 10).unwrap(), "3/10");
        assert_eq!(format_number(NumberFormat::PageXofN, 3, 10).unwrap(), "Page 3 of 10");
    }

    #[test]
    fn lower_and_upper_roman() {
        let cases = [(1, "i"), (4, "iv"), (9, "ix"), (40, "xl"), (90, "xc"), (2024, "mmxxiv")];
        for (n, want) in cases {
            assert_eq!(format_number(NumberFormat::LowerRoman, n, 0).unwrap(), want, "lower {n}");
            assert_eq!(
                format_number(NumberFormat::UpperRoman, n, 0).unwrap(),
                want.to_uppercase(),
                "upper {n}"
            );
        }
    }

    #[test]
    fn alpha_bijective_base26() {
        let cases = [(1, "a"), (2, "b"), (26, "z"), (27, "aa"), (28, "ab"), (52, "az"), (53, "ba")];
        for (n, want) in cases {
            assert_eq!(format_number(NumberFormat::LowerAlpha, n, 0).unwrap(), want, "lower {n}");
            assert_eq!(
                format_number(NumberFormat::UpperAlpha, n, 0).unwrap(),
                want.to_uppercase(),
                "upper {n}"
            );
        }
    }

    #[test]
    fn roman_and_alpha_reject_nonpositive() {
        assert!(format_number(NumberFormat::LowerRoman, 0, 0).is_err());
        assert!(format_number(NumberFormat::UpperRoman, -1, 0).is_err());
        assert!(format_number(NumberFormat::LowerAlpha, 0, 0).is_err());
        assert!(format_number(NumberFormat::UpperAlpha, -3, 0).is_err());
        // Decimal is defined for any value (though the caller enforces start >= 1).
        assert_eq!(format_number(NumberFormat::Decimal, 0, 5).unwrap(), "0");
    }

    #[test]
    fn parse_round_trips_wire_strings() {
        for s in [
            "decimal", "decimal-slash-total", "page-x-of-n", "lower-roman", "upper-roman",
            "lower-alpha", "upper-alpha",
        ] {
            assert!(NumberFormat::parse(s).is_ok(), "{s}");
        }
        assert!(NumberFormat::parse("bogus").is_err());
    }
}
