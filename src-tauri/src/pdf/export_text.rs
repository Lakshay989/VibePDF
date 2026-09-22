//! SPEC: P7-OCR-006 — export a document as plain text, in reading order, as
//! UTF-8.
//!
//! **Why this is not `page.text().all()`.** `PDFium` returns text in content-
//! stream order, which is usually reading order and is wrong exactly where it
//! matters. Measured on `tests/fixtures/basic/two-column.pdf` (2026-09-23),
//! `PDFium` gives:
//!
//! ```text
//! Alpha one Beta one
//! Alpha two Beta two
//! ```
//!
//! — the two columns merged line by line, which is not how anyone reads a
//! two-column page. So the text is rebuilt from the page's runs and their
//! positions: split into columns, then lines, then left to right.
//!
//! The ordering ([`order_pieces`]) is a pure function over rectangles, so it is
//! unit-tested directly with synthetic runs; `PDFium` is only the source of
//! those rectangles.
//!
//! Known limits, stated rather than implied: text is ordered geometrically, so
//! a sidebar or a pull-quote lands where its position says, not where an
//! author would want it; right-to-left scripts come out in visual order;
//! hyphenation and ligatures are left as the document wrote them.

use pdfium_render::prelude::{PdfDocument, PdfPageObjectCommon, PdfPageObjectsCommon};
use serde::Serialize;

use crate::error::CommandError;
use crate::pdf::cos::visual_transform;
use crate::pdf::document::pdfium_lock;

/// One run of text with where it sits, in the space the reader sees (after
/// `/Rotate`), y growing *downwards* so "first" is smaller.
#[derive(Debug, Clone, PartialEq)]
pub struct TextPiece {
    pub text: String,
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}

impl TextPiece {
    fn middle_y(&self) -> f32 {
        (self.top + self.bottom) / 2.0
    }

    fn height(&self) -> f32 {
        (self.bottom - self.top).abs()
    }
}

/// What an export produced, for the caller and the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextExportSummary {
    pub pages: u32,
    pub characters: u64,
}

/// A gap this wide between blocks of text is a column boundary rather than
/// word spacing. Two points is a space; two inches is a different column.
const COLUMN_GAP: f32 = 36.0;
/// More gaps than this and the page is a table or a form, not columns — the
/// geometry stops meaning "read this side first", so fall back to line order.
const MAX_COLUMNS: usize = 4;

/// SPEC: P7-OCR-006 — order one page's runs the way a person reads them.
///
/// Columns first (left to right), then lines down each column, then runs across
/// each line. Pure: rectangles in, text out.
#[must_use]
pub fn order_pieces(pieces: &[TextPiece]) -> String {
    if pieces.is_empty() {
        return String::new();
    }
    let columns = split_columns(pieces);
    let mut out = String::new();
    for column in columns {
        for line in split_lines(&column) {
            let mut sorted = line;
            sorted.sort_by(|a, b| a.left.total_cmp(&b.left));
            let mut text = String::new();
            for piece in sorted {
                // PDF has no spaces, only positions. A run that starts well
                // after the previous one ended is a word break; anything
                // tighter is the same word split across runs.
                if !text.is_empty() && !text.ends_with(' ') && !piece.text.starts_with(' ') {
                    text.push(' ');
                }
                text.push_str(&piece.text);
            }
            let trimmed = text.trim_end();
            if !trimmed.is_empty() {
                out.push_str(trimmed);
                out.push('\n');
            }
        }
    }
    out
}

/// Group runs into columns by looking for vertical corridors with no text in
/// them. Returns one group when the page has no such corridor.
fn split_columns(pieces: &[TextPiece]) -> Vec<Vec<&TextPiece>> {
    let mut spans: Vec<(f32, f32)> = pieces.iter().map(|p| (p.left, p.right)).collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));

    // Merge overlapping spans; what is left between them are the corridors.
    let mut merged: Vec<(f32, f32)> = Vec::new();
    for (left, right) in spans {
        match merged.last_mut() {
            Some(last) if left <= last.1 + COLUMN_GAP => last.1 = last.1.max(right),
            _ => merged.push((left, right)),
        }
    }
    if merged.len() < 2 || merged.len() > MAX_COLUMNS {
        return vec![pieces.iter().collect()];
    }

    merged
        .iter()
        .map(|(left, right)| {
            pieces
                .iter()
                .filter(|p| {
                    let centre = (p.left + p.right) / 2.0;
                    centre >= *left && centre <= *right
                })
                .collect()
        })
        .collect()
}

/// Group a column's runs into lines by vertical position, top first.
fn split_lines<'a>(column: &[&'a TextPiece]) -> Vec<Vec<&'a TextPiece>> {
    let mut sorted: Vec<&TextPiece> = column.to_vec();
    sorted.sort_by(|a, b| a.middle_y().total_cmp(&b.middle_y()));

    let mut lines: Vec<Vec<&TextPiece>> = Vec::new();
    for piece in sorted {
        // Half a line's height: enough to hold a line together through
        // superscripts and mixed sizes, tight enough to keep lines apart.
        let tolerance = (piece.height() / 2.0).max(1.0);
        match lines.last_mut() {
            Some(line)
                if line
                    .first()
                    .is_some_and(|first| (first.middle_y() - piece.middle_y()).abs() <= tolerance) =>
            {
                line.push(piece);
            }
            _ => lines.push(vec![piece]),
        }
    }
    lines
}

/// Read one page's runs, in the space the reader sees.
fn page_pieces(doc: &PdfDocument<'_>, index: i32) -> Result<Vec<TextPiece>, CommandError> {
    let page = doc.pages().get(index).map_err(CommandError::from)?;
    #[allow(clippy::cast_possible_truncation)] // /Rotate is a multiple of 90
    let rotate = page.rotation().map_err(CommandError::from)?.as_degrees() as i64;
    let (width, height) = (page.width().value, page.height().value);
    let (_, vw, vh) = visual_transform(rotate, [0.0, 0.0, width, height]);

    let mut pieces = Vec::new();
    for object in page.objects().iter() {
        let Some(text_object) = object.as_text_object() else {
            continue;
        };
        let text = text_object.text();
        if text.trim().is_empty() {
            continue;
        }
        let Ok(bounds) = object.bounds() else { continue };
        // Page space has y up and ignores /Rotate; put both corners into the
        // displayed frame with y down, which is the order-of-reading frame.
        let (x0, y0) = to_visual(rotate, vw, vh, bounds.left().value, bounds.top().value);
        let (x1, y1) = to_visual(rotate, vw, vh, bounds.right().value, bounds.bottom().value);
        pieces.push(TextPiece {
            text,
            left: x0.min(x1),
            right: x0.max(x1),
            top: y0.min(y1),
            bottom: y0.max(y1),
        });
    }
    Ok(pieces)
}

/// Page space (y up, unrotated) → the displayed frame (y down).
fn to_visual(rotate: i64, vw: f32, vh: f32, x: f32, y: f32) -> (f32, f32) {
    match rotate {
        90 => (vw - y, vh - x),
        180 => (vw - x, y),
        270 => (y, x),
        _ => (x, vh - y),
    }
}

/// SPEC: P7-OCR-006 — the whole document (or `pages`) as text.
///
/// Pages are separated by a blank line. A page with no text contributes
/// nothing — a scan reads as empty until OCR has run over it, which is a fact
/// about the document, not a failure.
///
/// # Errors
/// When a page cannot be read.
pub fn document_text(
    doc: &PdfDocument<'_>,
    pages: &[usize],
) -> Result<(String, TextExportSummary), CommandError> {
    let _guard = pdfium_lock()?;
    let count = doc.pages().len();
    let wanted: Vec<i32> = if pages.is_empty() {
        (0..count).collect()
    } else {
        pages
            .iter()
            .map(|&p| {
                i32::try_from(p)
                    .ok()
                    .filter(|index| *index < count)
                    .ok_or_else(|| CommandError::InvalidInput(format!("page out of range: {p}")))
            })
            .collect::<Result<_, _>>()?
    };

    let mut out = String::new();
    for index in &wanted {
        let page = order_pieces(&page_pieces(doc, *index)?);
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&page);
    }

    let summary = TextExportSummary {
        pages: u32::try_from(wanted.len()).unwrap_or(u32::MAX),
        characters: out.chars().count() as u64,
    };
    Ok((out, summary))
}

#[allow(clippy::expect_used, clippy::unwrap_used)]
#[cfg(test)]
mod order_tests {
    use super::{order_pieces, TextPiece};

    fn piece(text: &str, left: f32, top: f32) -> TextPiece {
        TextPiece {
            text: text.to_owned(),
            left,
            right: left + 60.0,
            top,
            bottom: top + 12.0,
        }
    }

    #[test]
    fn a_single_column_reads_top_to_bottom() {
        let pieces = vec![piece("second", 72.0, 120.0), piece("first", 72.0, 100.0)];
        assert_eq!(order_pieces(&pieces), "first\nsecond\n");
    }

    #[test]
    fn two_columns_read_one_after_the_other() {
        // The failure this exists to catch: PDFium returns these interleaved,
        // "Alpha one Beta one", because that is the order they were drawn.
        let mut pieces = Vec::new();
        for row in 0..3u8 {
            let y = 100.0 + f32::from(row) * 28.0;
            pieces.push(piece(&format!("Alpha {row}"), 72.0, y));
            pieces.push(piece(&format!("Beta {row}"), 330.0, y));
        }
        assert_eq!(
            order_pieces(&pieces),
            "Alpha 0\nAlpha 1\nAlpha 2\nBeta 0\nBeta 1\nBeta 2\n"
        );
    }

    #[test]
    fn runs_on_one_line_join_left_to_right() {
        let pieces = vec![
            TextPiece {
                text: "world".into(),
                left: 140.0,
                right: 190.0,
                top: 100.0,
                bottom: 112.0,
            },
            TextPiece {
                text: "hello".into(),
                left: 72.0,
                right: 130.0,
                top: 100.0,
                bottom: 112.0,
            },
        ];
        assert_eq!(order_pieces(&pieces), "hello world\n");
    }

    #[test]
    fn a_run_that_already_ends_in_a_space_is_not_given_another() {
        let pieces = vec![
            TextPiece {
                text: "hello ".into(),
                left: 72.0,
                right: 130.0,
                top: 100.0,
                bottom: 112.0,
            },
            TextPiece {
                text: "world".into(),
                left: 132.0,
                right: 190.0,
                top: 100.0,
                bottom: 112.0,
            },
        ];
        assert_eq!(order_pieces(&pieces), "hello world\n");
    }

    #[test]
    fn a_busy_page_falls_back_to_line_order() {
        // Five columns is a table or a form. Reading it column-first would
        // scramble the rows, so the geometry stops being trusted.
        let mut pieces = Vec::new();
        for column in 0..5u8 {
            for row in 0..2u8 {
                pieces.push(piece(
                    &format!("c{column}r{row}"),
                    72.0 + f32::from(column) * 100.0,
                    100.0 + f32::from(row) * 28.0,
                ));
            }
        }
        let text = order_pieces(&pieces);
        assert!(text.starts_with("c0r0 c1r0 c2r0 c3r0 c4r0\n"), "was: {text}");
    }

    #[test]
    fn nothing_in_nothing_out() {
        assert_eq!(order_pieces(&[]), "");
    }
}

#[allow(clippy::expect_used, clippy::unwrap_used)]
#[cfg(test)]
mod fixture_builder {
    //! Rebuilds `tests/fixtures/basic/unicode-text.pdf`: Cyrillic and Greek as
    //! real, selectable text. Here rather than in a Python generator because
    //! the base-14 fonts cannot encode either script — it needs the crate's own
    //! CID embedding, the same path `scan-cyrillic.pdf` uses before rasterising.

    use lopdf::{Dictionary, Document, Object};

    use crate::pdf::font_embed_cid::{build_cid_font, place_cid_run, CidRun};
    use crate::pdf::font_resolver::covering_font_bytes;

    const LINES: &[&str] = &[
        "Здравствуйте, мир",
        "Καλημέρα κόσμε",
        "Grüße, Welt — naïve café",
    ];

    #[test]
    #[ignore = "regenerates a committed fixture; run on demand"]
    fn writes_the_unicode_text_fixture() {
        let text: String = LINES.concat();
        let font_bytes = covering_font_bytes(&text).expect("a covering system face");

        let mut doc = Document::with_version("1.5");
        let tree_id = doc.new_object_id();
        let leaf_id = doc.new_object_id();
        let mut page = Dictionary::new();
        page.set("Type", Object::Name(b"Page".to_vec()));
        page.set("Parent", Object::Reference(tree_id));
        page.set(
            "MediaBox",
            Object::Array(vec![0.into(), 0.into(), Object::Real(612.0), Object::Real(792.0)]),
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

        let cid = build_cid_font(&mut doc, &font_bytes, &text).expect("embed");
        let font_name = crate::pdf::cos::register_page_resource(
            &mut doc,
            leaf_id,
            b"Font",
            "Fu",
            Object::Dictionary(cid.font_dict.clone()),
        )
        .expect("register");
        let mut y = 700.0f32;
        for line in LINES {
            place_cid_run(
                &mut doc,
                leaf_id,
                &font_name,
                &cid,
                &CidRun {
                    text: line,
                    size: 18.0,
                    color: (0.0, 0.0, 0.0),
                    matrix: [1.0, 0.0, 0.0, 1.0, 72.0, y],
                    opacity: 1.0,
                    behind: false,
                    underline: None,
                    kind: "fixture",
                },
            )
            .expect("place");
            y -= 40.0;
        }

        let target = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/fixtures/basic/unicode-text.pdf");
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("save");
        std::fs::write(&target, &bytes).expect("write fixture");
        println!("wrote {} ({} bytes)", target.display(), bytes.len());
    }
}
