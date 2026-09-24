//! Reading one page into the pieces every exporter needs: styled text runs,
//! images, and the rules a table is detected from.
//!
//! SPEC: P7-OCR-004, P7-OCR-008. Lifted out of `export_docx` in P7.B5, when a
//! second exporter needed the same three things. Word and Excel then differ
//! only in what they *do* with them — Word keeps the formatting, Excel keeps
//! the values — rather than each owning a copy of the reading.
//!
//! The one thing here that is not obvious is the coordinate space. `PDFium`
//! reports a path segment's point **before** the object matrix is applied,
//! while its bounding box is after: measured 2026-09-23, a line written
//! `10 10 m 100 10 l` under `2 0 0 2 20 40 cm` reports its points as
//! (10,10)-(100,10) and its bounds as (38.5,58.5)-(221.5,61.5). Anything that
//! trusts the raw points mislocates every rule in a transformed document, and
//! most real documents are transformed.

use pdfium_render::prelude::{
    PdfDocument, PdfPageObjectCommon, PdfPageObjectsCommon, PdfPathSegmentType, PdfPathSegments,
};

use crate::error::CommandError;
use crate::pdf::cos::visual_transform;
use crate::pdf::export_text::{to_visual, TextPiece};
use crate::pdf::table_detect::{usable, Cell, Rule};

/// An image, ready to become a `word/media` part.
#[derive(Clone, Debug)]
pub struct EmbeddedImage {
    /// PNG bytes.
    pub png: Vec<u8>,
    /// Drawn size in PDF points, which becomes the size in the document.
    pub width_points: f32,
    pub height_points: f32,
}


/// One styled run with where it sits, in the frame a reader sees.
#[derive(Clone)]
pub struct Placed {
    pub piece: TextPiece,
    pub bold: bool,
    pub italic: bool,
    pub size: f32,
}

/// Weight at or above which `PDFium` calls a font bold. 600 is semibold; Word
/// has one bold, so semibold and heavier both become it.
const BOLD_WEIGHT: i32 = 600;

/// `PdfFontWeight` is an enum of the nine standard steps plus `Custom`; this
/// flattens it back to the number the comparison needs.
fn numeric_weight(weight: pdfium_render::prelude::PdfFontWeight) -> i32 {
    use pdfium_render::prelude::PdfFontWeight as W;
    match weight {
        W::Weight100 => 100,
        W::Weight200 => 200,
        W::Weight300 => 300,
        W::Weight400Normal => 400,
        W::Weight500 => 500,
        W::Weight600 => 600,
        W::Weight700Bold => 700,
        W::Weight800 => 800,
        W::Weight900 => 900,
        W::Custom(value) => i32::try_from(value).unwrap_or(i32::MAX),
    }
}

/// Is this font bold or italic? Flags first, name second.
///
/// The name is not a fallback for tidiness: plenty of generators embed a font
/// called `Arial-BoldMT` whose descriptor carries no usable weight at all, and
/// dropping the emphasis from those documents is a visible loss.
pub(crate) fn font_style(name: &str, weight: Option<i32>, italic_flag: bool, italic_angle: i32) -> (bool, bool) {
    let lower = name.to_ascii_lowercase();
    let bold = weight.is_some_and(|w| w >= BOLD_WEIGHT)
        || lower.contains("bold")
        || lower.contains("black")
        || lower.contains("heavy");
    let italic = italic_flag
        || italic_angle != 0
        || lower.contains("italic")
        || lower.contains("oblique");
    (bold, italic)
}

/// What one page contributes: its styled runs, its images, and the rules a
/// table might be detected from.
pub type PageContent = (Vec<Placed>, Vec<EmbeddedImage>, Vec<Rule>);

/// Read one page into styled runs, images and rules, in the displayed frame.
pub fn page_content(doc: &PdfDocument<'_>, index: i32) -> Result<PageContent, CommandError> {

    let page = doc.pages().get(index).map_err(CommandError::from)?;
    #[allow(clippy::cast_possible_truncation)] // /Rotate is a multiple of 90
    let rotate = page.rotation().map_err(CommandError::from)?.as_degrees() as i64;
    let (width, height) = (page.width().value, page.height().value);
    let (_, vw, vh) = visual_transform(rotate, [0.0, 0.0, width, height]);

    let mut runs = Vec::new();
    let mut images = Vec::new();
    let mut rules = Vec::new();
    for object in page.objects().iter() {
        let Ok(bounds) = object.bounds() else { continue };
        let (x0, y0) = to_visual(rotate, vw, vh, bounds.left().value, bounds.top().value);
        let (x1, y1) = to_visual(rotate, vw, vh, bounds.right().value, bounds.bottom().value);
        let (left, right) = (x0.min(x1), x0.max(x1));
        let (top, bottom) = (y0.min(y1), y0.max(y1));

        if let Some(text_object) = object.as_text_object() {
            let text = text_object.text();
            if text.trim().is_empty() {
                continue;
            }
            let font = text_object.font();
            let (bold, italic) = font_style(
                &font.name(),
                font.weight().ok().map(numeric_weight),
                font.is_italic(),
                font.italic_angle().unwrap_or(0),
            );
            runs.push(Placed {
                piece: TextPiece { text, left, right, top, bottom },
                bold,
                italic,
                size: text_object.scaled_font_size().value,
            });
        } else if let Some(image_object) = object.as_image_object() {
            // The drawn size, not the pixel size: an image belongs in the
            // document at the size it occupied on the page.
            let Ok(image) = image_object.get_raw_image() else {
                continue;
            };
            let rgba = image.to_rgba8();
            let Ok(png) = crate::pdf::render::encode_png(rgba.width(), rgba.height(), rgba.as_raw())
            else {
                continue;
            };
            images.push(EmbeddedImage {
                png,
                width_points: (right - left).abs().max(1.0),
                height_points: (bottom - top).abs().max(1.0),
            });
        } else if let Some(path) = object.as_path_object() {
            // SPEC: P7-OCR-004 — the rules a table is detected from.
            //
            // Segment points are in the object's **own** space, not the page's:
            // measured 2026-09-23, a line written `10 10 m 100 10 l` under a
            // `2 0 0 2 20 40 cm` reports its points as (10,10)-(100,10) while
            // its bounds report (38.5,58.5)-(221.5,61.5). So the object matrix
            // has to be applied, and a detector that trusted the raw points
            // would mislocate every rule in any document that uses a transform.
            let matrix = object.matrix().ok();
            let apply = |x: f32, y: f32| -> (f32, f32) {
                matrix.map_or((x, y), |m| {
                    (
                        m.a().mul_add(x, m.c() * y) + m.e(),
                        m.b().mul_add(x, m.d() * y) + m.f(),
                    )
                })
            };

            let mut previous: Option<(f32, f32)> = None;
            for segment in path.segments().iter() {
                let (px, py) = segment.point();
                let point = apply(px.value, py.value);
                let is_line = segment.segment_type() == PdfPathSegmentType::LineTo;
                if let (true, Some(start)) = (is_line, previous) {
                    if let Some(rule) = rule_between(start, point, rotate, vw, vh) {
                        rules.push(rule);
                    }
                }
                previous = Some(point);
            }
        }
    }
    Ok((runs, images, usable(&rules)))
}

/// Two points make a rule when the line between them is axis-aligned. Anything
/// else — a diagonal, a curve's control polygon — is not part of a grid.
fn rule_between(
    start: (f32, f32),
    end: (f32, f32),
    rotate: i64,
    vw: f32,
    vh: f32,
) -> Option<Rule> {
    // Straightness is judged in page space, before the y-flip, so the test is
    // the same one the content stream wrote.
    let flat_y = (start.1 - end.1).abs() <= STRAIGHT_TOLERANCE;
    let flat_x = (start.0 - end.0).abs() <= STRAIGHT_TOLERANCE;
    if flat_y == flat_x {
        return None; // a point, or a diagonal
    }
    let a = to_visual(rotate, vw, vh, start.0, start.1);
    let b = to_visual(rotate, vw, vh, end.0, end.1);
    // A page turned 90 degrees turns rows into columns, so which axis a rule
    // lies on is decided *after* the rotation, not before.
    let horizontal = (a.1 - b.1).abs() <= STRAIGHT_TOLERANCE;
    Some(if horizontal {
        Rule { horizontal: true, position: (a.1 + b.1) / 2.0, from: a.0.min(b.0), to: a.0.max(b.0) }
    } else {
        Rule { horizontal: false, position: (a.0 + b.0) / 2.0, from: a.1.min(b.1), to: a.1.max(b.1) }
    })
}

/// How far from axis-aligned a rule may be and still count. Half a point
/// absorbs rounding in the content stream without admitting a real diagonal.
const STRAIGHT_TOLERANCE: f32 = 0.5;


/// Is this run drawn inside this cell? By its centre, so a glyph that overhangs
/// a rule by a hair still belongs to the cell it was written in.
#[must_use]
pub fn in_cell(cell: &Cell, piece: &TextPiece) -> bool {
    let x = (piece.left + piece.right) / 2.0;
    let y = (piece.top + piece.bottom) / 2.0;
    x >= cell.left && x <= cell.right && y >= cell.top && y <= cell.bottom
}


#[allow(clippy::expect_used, clippy::unwrap_used, clippy::float_cmp)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_style_reads_flags_first_and_the_name_second() {
        // Flags when they are there.
        assert_eq!(font_style("Helvetica", Some(700), false, 0), (true, false));
        assert_eq!(font_style("Helvetica", Some(400), true, 0), (false, true));
        assert_eq!(font_style("Helvetica", Some(400), false, -12), (false, true));
        // Semibold counts as bold: Word has one bold.
        assert_eq!(font_style("Helvetica", Some(600), false, 0), (true, false));
        assert_eq!(font_style("Helvetica", Some(500), false, 0), (false, false));
        // And the name when the descriptor says nothing — plenty of generators
        // ship `Arial-BoldMT` with no usable weight at all.
        assert_eq!(font_style("Arial-BoldMT", None, false, 0), (true, false));
        assert_eq!(font_style("Helvetica-Oblique", None, false, 0), (false, true));
        assert_eq!(font_style("Foo-BoldItalic", None, false, 0), (true, true));
        assert_eq!(font_style("Helvetica", None, false, 0), (false, false));
    }


    #[test]
    fn two_points_make_a_rule_only_when_the_line_is_straight() {
        // Page is 612x792 unrotated, so `to_visual` just flips y.
        let horizontal = rule_between((72.0, 700.0), (500.0, 700.0), 0, 612.0, 792.0)
            .expect("a horizontal line is a rule");
        assert!(horizontal.horizontal);
        assert_eq!(horizontal.position, 92.0); // 792 - 700
        assert_eq!((horizontal.from, horizontal.to), (72.0, 500.0));

        let vertical = rule_between((72.0, 700.0), (72.0, 500.0), 0, 612.0, 792.0)
            .expect("a vertical line is a rule");
        assert!(!vertical.horizontal);
        assert_eq!(vertical.position, 72.0);

        // A diagonal is not part of a grid, and neither is a zero-length hop.
        assert!(rule_between((72.0, 700.0), (500.0, 500.0), 0, 612.0, 792.0).is_none());
        assert!(rule_between((72.0, 700.0), (72.0, 700.0), 0, 612.0, 792.0).is_none());
    }

    #[test]
    fn a_rule_is_straight_within_half_a_point() {
        // Content streams round their coordinates; a hairline off-axis is still
        // a rule, a visible slope is not.
        assert!(rule_between((72.0, 700.0), (500.0, 700.3), 0, 612.0, 792.0).is_some());
        assert!(rule_between((72.0, 700.0), (500.0, 706.0), 0, 612.0, 792.0).is_none());
    }

    #[test]
    fn a_turned_page_turns_rows_into_columns() {
        // Which axis a rule lies on is decided *after* the rotation. The same
        // line is a row on an upright page and a column on one turned 90°.
        let upright = rule_between((72.0, 700.0), (500.0, 700.0), 0, 612.0, 792.0).expect("rule");
        let turned = rule_between((72.0, 700.0), (500.0, 700.0), 90, 792.0, 612.0).expect("rule");
        assert!(upright.horizontal);
        assert!(!turned.horizontal, "a rotated page did not swap the axis");
    }

    #[test]
    fn a_run_belongs_to_the_cell_its_centre_sits_in() {
        let cell = Cell { row: 0, column: 0, span: 1, left: 72.0, right: 220.0, top: 100.0, bottom: 124.0 };
        let piece = |left: f32, top: f32| TextPiece {
            text: "x".into(), left, right: left + 30.0, top, bottom: top + 10.0,
        };
        assert!(in_cell(&cell, &piece(80.0, 105.0)));
        // A glyph overhanging a rule still belongs to the cell it was written
        // in, because the test is on its centre.
        assert!(in_cell(&cell, &piece(200.0, 105.0)));
        assert!(!in_cell(&cell, &piece(230.0, 105.0)), "a run in the next column");
        assert!(!in_cell(&cell, &piece(80.0, 130.0)), "a run in the next row");
    }
}
