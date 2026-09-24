//! SPEC: P7-OCR-004 — export a document as Word (`.docx`), preserving text,
//! bold/italic, headings and images.
//!
//! **Tables are not here.** P7-OCR-004's "table structures where detectable" is
//! P7.B1b: detecting a table is a different problem with a different failure
//! mode, because a *wrong* table is worse for a reader than no table at all.
//!
//! **Reading order is not reimplemented.** [`crate::pdf::export_text`] already
//! solves it — columns left to right, lines down each column — and is unit
//! tested as a pure function over rectangles. Its `group_pieces` returns
//! indices rather than text precisely so this module can carry each run's font
//! and size through the same ordering instead of keeping a second copy that
//! drifts.
//!
//! **Headings are relative, never absolute.** There is no "heading" flag in a
//! PDF; there is only type that is bigger than the type around it. So the body
//! size is whatever size is most common in the document, and a short line set
//! materially larger than that is a heading. A document set entirely in one
//! size gets no headings at all — inventing them from nothing would be worse
//! than leaving the text flat.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use pdfium_render::prelude::{
    PdfDocument, PdfPageObjectCommon, PdfPageObjectsCommon, PdfPathSegmentType, PdfPathSegments,
};
use serde::Serialize;

use crate::error::CommandError;
use crate::pdf::export_text::{group_pieces, TextPiece};
use crate::pdf::ooxml::{escape_xml, ZipWriter};
use crate::pdf::table_detect::{detect_tables, usable, Cell, Rule, Table};

/// One run of text with the formatting Word needs for it.
#[derive(Clone, Debug, PartialEq)]
pub struct StyledRun {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    /// Size in points, as drawn.
    pub size: f32,
}

/// A paragraph: its runs, and which Word style it takes.
#[derive(Clone, Debug, PartialEq)]
pub struct Paragraph {
    pub runs: Vec<StyledRun>,
    /// `None` for body text, `Some(1..=3)` for a heading level.
    pub heading: Option<u8>,
}

/// An image, ready to become a `word/media` part.
#[derive(Clone, Debug)]
pub struct EmbeddedImage {
    /// PNG bytes.
    pub png: Vec<u8>,
    /// Drawn size in PDF points, which becomes the size in the document.
    pub width_points: f32,
    pub height_points: f32,
}

/// What an export produced.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocxExportSummary {
    pub pages: u32,
    pub paragraphs: u32,
    pub headings: u32,
    pub images: u32,
    pub tables: u32,
    pub bytes: u64,
}

/// A line set this much larger than the body is a heading rather than emphasis.
/// 1.15 is deliberately low: many documents set headings only slightly larger,
/// and the shortness rule below is what keeps false positives down.
const HEADING_RATIO: f32 = 1.15;
/// A heading is a *title*, so it does not fill its column. A long line set in a
/// large face is much more likely to be a pull-quote or large body text.
///
/// Measured 2026-09-23: a plain 20 pt title over 9 pt body ran to 63% of its
/// column, so the first value tried here (0.6) rejected the most obvious
/// heading in the fixture. 0.8 still excludes body text, which wraps at ~100%.
const HEADING_MAX_WIDTH_SHARE: f32 = 0.8;
/// Above this multiple of the body size, the width rule is not applied at all.
/// The rule exists to catch type that is only *slightly* larger — a pull-quote,
/// a lead paragraph — where length is the tie-breaker. Type at more than one
/// and a half times the body is a heading whatever its length.
const HEADING_OBVIOUS_RATIO: f32 = 1.5;
/// Sizes are bucketed to a tenth of a point before being counted, so that runs
/// that differ only by floating-point noise agree on the body size.
const SIZE_BUCKET: f32 = 10.0;

/// PDF points to English Metric Units, which is what `DrawingML` measures in.
/// 1 pt = 12,700 EMU, exactly.
#[must_use]
pub fn points_to_emu(points: f32) -> i64 {
    #[allow(clippy::cast_possible_truncation)]
    let emu = (f64::from(points) * 12_700.0).round() as i64;
    emu.max(1)
}

/// Bucket a size so near-identical sizes count as one.
fn bucket(size: f32) -> i32 {
    #[allow(clippy::cast_possible_truncation)]
    let v = (size * SIZE_BUCKET).round() as i32;
    v
}

/// SPEC: P7-OCR-004 — the document's body size: the size the most characters
/// are set in.
///
/// Counted by **characters, not by runs**. A page of body text is a handful of
/// long runs and a heading is one short one, so counting runs would let a
/// three-word title outvote a paragraph.
#[must_use]
pub fn body_size(runs: &[StyledRun]) -> Option<f32> {
    let mut weight: BTreeMap<i32, usize> = BTreeMap::new();
    for run in runs {
        let chars = run.text.chars().filter(|c| !c.is_whitespace()).count();
        if chars > 0 {
            *weight.entry(bucket(run.size)).or_default() += chars;
        }
    }
    weight
        .into_iter()
        // Ties go to the smaller size: body text is more common than display
        // type, and guessing large would turn a whole document into headings.
        .max_by_key(|(size, count)| (*count, -*size))
        .map(|(size, _)| {
            // Sizes are bucketed tenths of a point, so the value is small and
            // the cast is exact for every real font size.
            #[allow(clippy::cast_precision_loss)]
            let size = size as f32;
            size / SIZE_BUCKET
        })
}

/// SPEC: P7-OCR-004 — which heading level a line takes, if any.
///
/// `sizes` is every distinct heading-sized bucket in the document, largest
/// first; a line's rank in that list is its level, capped at 3.
#[must_use]
pub fn heading_level(
    line_size: f32,
    line_width: f32,
    column_width: f32,
    body: f32,
    sizes: &[i32],
) -> Option<u8> {
    if body <= 0.0 || line_size < body * HEADING_RATIO {
        return None;
    }
    if line_size < body * HEADING_OBVIOUS_RATIO
        && column_width > 0.0
        && line_width > column_width * HEADING_MAX_WIDTH_SHARE
    {
        return None;
    }
    let rank = sizes.iter().position(|s| *s == bucket(line_size))?;
    #[allow(clippy::cast_possible_truncation)]
    let level = (rank as u8).saturating_add(1).min(3);
    Some(level)
}

/// The distinct heading-sized buckets in a document, largest first.
#[must_use]
pub fn heading_sizes(runs: &[StyledRun], body: f32) -> Vec<i32> {
    let mut sizes: Vec<i32> = runs
        .iter()
        .filter(|r| r.size >= body * HEADING_RATIO && !r.text.trim().is_empty())
        .map(|r| bucket(r.size))
        .collect();
    sizes.sort_unstable_by(|a, b| b.cmp(a));
    sizes.dedup();
    sizes
}

/// The five fixed parts of a `.docx`, plus one relationship per image.
///
/// Word is strict about these: a missing content type or a relationship that
/// points nowhere produces "the file is corrupt", with no indication which of
/// the parts is at fault.
fn content_types(image_count: usize) -> String {
    let png = if image_count > 0 {
        r#"<Default Extension="png" ContentType="image/png"/>"#
    } else {
        ""
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">\
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>\
<Default Extension="xml" ContentType="application/xml"/>{png}\
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>\
<Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>\
</Types>"#
    )
}

const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;

/// Heading styles, so Word's navigation pane and table of contents see them as
/// headings rather than as merely large text.
fn styles_xml() -> String {
    let mut out = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>"#,
    );
    for level in 1..=3 {
        // Sizes are in half-points, which is what w:sz means.
        let size = match level {
            1 => 32,
            2 => 26,
            _ => 24,
        };
        let _ = write!(
            out,
            r#"<w:style w:type="paragraph" w:styleId="Heading{level}"><w:name w:val="heading {level}"/><w:basedOn w:val="Normal"/><w:uiPriority w:val="9"/><w:qFormat/><w:pPr><w:outlineLvl w:val="{outline}"/></w:pPr><w:rPr><w:b/><w:sz w:val="{size}"/></w:rPr></w:style>"#,
            outline = level - 1
        );
    }
    // The style `table_xml` names. Word tolerates a missing style reference,
    // but LibreOffice renders the table without borders, so it is defined.
    out.push_str(
        r#"<w:style w:type="table" w:styleId="TableGrid"><w:name w:val="Table Grid"/><w:tblPr><w:tblBorders><w:top w:val="single" w:sz="4" w:color="auto"/><w:left w:val="single" w:sz="4" w:color="auto"/><w:bottom w:val="single" w:sz="4" w:color="auto"/><w:right w:val="single" w:sz="4" w:color="auto"/><w:insideH w:val="single" w:sz="4" w:color="auto"/><w:insideV w:val="single" w:sz="4" w:color="auto"/></w:tblBorders></w:tblPr></w:style>"#,
    );
    out.push_str("</w:styles>");
    out
}

/// One paragraph as `WordprocessingML`.
fn paragraph_xml(paragraph: &Paragraph) -> String {
    let mut out = String::from("<w:p>");
    if let Some(level) = paragraph.heading {
        let _ = write!(out, r#"<w:pPr><w:pStyle w:val="Heading{level}"/></w:pPr>"#);
    }
    for run in &paragraph.runs {
        out.push_str("<w:r>");
        if run.bold || run.italic {
            out.push_str("<w:rPr>");
            if run.bold {
                out.push_str("<w:b/>");
            }
            if run.italic {
                out.push_str("<w:i/>");
            }
            out.push_str("</w:rPr>");
        }
        // xml:space="preserve" or Word eats the leading and trailing spaces
        // that carry word boundaries between runs.
        let _ = write!(
            out,
            r#"<w:t xml:space="preserve">{}</w:t>"#,
            escape_xml(&run.text)
        );
        out.push_str("</w:r>");
    }
    out.push_str("</w:p>");
    out
}

/// One image as an inline `DrawingML` drawing, sized from the space it filled
/// on the page.
fn image_xml(index: usize, image: &EmbeddedImage) -> String {
    let id = index + 1;
    let (cx, cy) = (
        points_to_emu(image.width_points),
        points_to_emu(image.height_points),
    );
    format!(
        r#"<w:p><w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="{cx}" cy="{cy}"/><wp:docPr id="{id}" name="Image {id}"/><a:graphic xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:pic xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><pic:nvPicPr><pic:cNvPr id="{id}" name="Image {id}"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="rId{rel}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"#,
        // rId1 is styles.xml, so images start at rId2.
        rel = index + 2
    )
}

/// One table as `WordprocessingML`.
///
/// Word wants an explicit grid (`w:tblGrid`) as well as the cells, and a cell
/// must contain at least one paragraph — an empty `w:tc` is one of the ways to
/// get "the file is corrupt", so an empty cell gets an empty paragraph.
fn table_xml(table: &FilledTable) -> String {
    // A single width for every column. The PDF's own column widths are known,
    // but Word lays a table out from its grid and the text almost never fits
    // the original measure; an even grid that Word then autofits reads better
    // than a faithful one that clips.
    let width = if table.columns == 0 {
        0
    } else {
        // Fiftieths of a percent, which is what `pct` means here: 100% = 5000.
        5000 / i32::try_from(table.columns).unwrap_or(1)
    };
    let mut out = String::from(
        r#"<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="5000" w:type="pct"/><w:tblBorders><w:top w:val="single" w:sz="4" w:color="auto"/><w:left w:val="single" w:sz="4" w:color="auto"/><w:bottom w:val="single" w:sz="4" w:color="auto"/><w:right w:val="single" w:sz="4" w:color="auto"/><w:insideH w:val="single" w:sz="4" w:color="auto"/><w:insideV w:val="single" w:sz="4" w:color="auto"/></w:tblBorders></w:tblPr><w:tblGrid>"#,
    );
    for _ in 0..table.columns {
        let _ = write!(out, r#"<w:gridCol w:w="{width}"/>"#);
    }
    out.push_str("</w:tblGrid>");

    for row in &table.rows {
        out.push_str("<w:tr>");
        for cell in row {
            let _ = write!(
                out,
                r#"<w:tc><w:tcPr><w:tcW w:w="{width}" w:type="pct"/></w:tcPr>"#
            );
            if cell.is_empty() {
                // Word requires a paragraph in every cell.
                out.push_str("<w:p/>");
            } else {
                for paragraph in cell {
                    out.push_str(&paragraph_xml(paragraph));
                }
            }
            out.push_str("</w:tc>");
        }
        out.push_str("</w:tr>");
    }
    out.push_str("</w:tbl>");
    // A table immediately followed by another, or ending the body, needs a
    // paragraph after it or Word merges them.
    out.push_str("<w:p/>");
    out
}

/// The whole `word/document.xml`.
fn document_xml(blocks: &[Block]) -> String {
    let mut out = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"><w:body>"#,
    );
    let mut image_index = 0;
    for block in blocks {
        match block {
            Block::Paragraph(p) => out.push_str(&paragraph_xml(p)),
            Block::Table(table) => out.push_str(&table_xml(table)),
            Block::Image(image) => {
                out.push_str(&image_xml(image_index, image));
                image_index += 1;
            }
        }
    }
    out.push_str("</w:body></w:document>");
    out
}

/// `word/_rels/document.xml.rels`: styles, then one entry per image.
fn document_rels(image_count: usize) -> String {
    let mut out = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#,
    );
    for i in 0..image_count {
        let _ = write!(
            out,
            r#"<Relationship Id="rId{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/image{}.png"/>"#,
            i + 2,
            i + 1
        );
    }
    out.push_str("</Relationships>");
    out
}

/// A document is a sequence of paragraphs and images, in reading order.
#[derive(Clone, Debug)]
pub enum Block {
    Paragraph(Paragraph),
    Image(EmbeddedImage),
    Table(FilledTable),
}

/// A detected table with each cell's text put back into it.
#[derive(Clone, Debug)]
pub struct FilledTable {
    pub columns: usize,
    /// Row-major: `rows[r][c]` is the paragraphs of cell `(r, c)`.
    pub rows: Vec<Vec<Vec<Paragraph>>>,
}

/// SPEC: P7-OCR-004 — assemble the `.docx` archive.
///
/// # Errors
/// When a part cannot be compressed or the archive cannot be finished.
pub fn build_docx(blocks: &[Block]) -> Result<Vec<u8>, CommandError> {
    let images: Vec<&EmbeddedImage> = blocks
        .iter()
        .filter_map(|b| match b {
            Block::Image(i) => Some(i),
            Block::Paragraph(_) | Block::Table(_) => None,
        })
        .collect();

    let mut zip = ZipWriter::new();
    zip.add("[Content_Types].xml", content_types(images.len()).as_bytes(), true)?;
    zip.add("_rels/.rels", ROOT_RELS.as_bytes(), true)?;
    zip.add("word/document.xml", document_xml(blocks).as_bytes(), true)?;
    zip.add("word/styles.xml", styles_xml().as_bytes(), true)?;
    zip.add(
        "word/_rels/document.xml.rels",
        document_rels(images.len()).as_bytes(),
        true,
    )?;
    for (i, image) in images.iter().enumerate() {
        // Stored, not deflated: a PNG is already compressed, and deflating it
        // again costs time to make it very slightly larger.
        zip.add(&format!("word/media/image{}.png", i + 1), &image.png, false)?;
    }
    zip.finish()
}

/// One styled run with where it sits, in the frame a reader sees.
#[derive(Clone)]
struct Placed {
    piece: TextPiece,
    bold: bool,
    italic: bool,
    size: f32,
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
fn font_style(name: &str, weight: Option<i32>, italic_flag: bool, italic_angle: i32) -> (bool, bool) {
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
type PageContent = (Vec<Placed>, Vec<EmbeddedImage>, Vec<Rule>);

/// Read one page into styled runs, images and rules, in the displayed frame.
fn page_content(doc: &PdfDocument<'_>, index: i32) -> Result<PageContent, CommandError> {
    use crate::pdf::cos::visual_transform;
    use crate::pdf::export_text::to_visual;

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
    use crate::pdf::export_text::to_visual;
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

/// A vertical gap wider than this many line-heights ends a paragraph.
const PARAGRAPH_GAP: f32 = 1.5;

/// SPEC: P7-OCR-004 — turn one page's ordered lines into paragraphs.
///
/// A line joins the paragraph above it unless something says otherwise: a
/// vertical gap over [`PARAGRAPH_GAP`] line-heights, a change of heading level,
/// or a change of left edge (an indent starts a new paragraph, which is what an
/// indent is *for*).
/// [`paragraphs_from_lines`] with each paragraph's top edge, so blocks can be
/// interleaved with tables in reading order.
fn paragraphs_with_position(
    runs: &[Placed],
    columns: &[Vec<Vec<usize>>],
    body: f32,
    sizes: &[i32],
) -> Vec<(f32, Paragraph)> {
    let tops = column_line_tops(runs, columns);
    let paragraphs = paragraphs_from_lines(runs, columns, body, sizes);
    // `paragraphs_from_lines` merges lines into paragraphs, so there are at
    // most as many paragraphs as lines; pairing in order is exact because both
    // walk the columns and lines in the same order.
    paragraphs
        .into_iter()
        .enumerate()
        .map(|(i, p)| (tops.get(i).copied().unwrap_or(0.0), p))
        .collect()
}

/// The top edge of each paragraph, in the order `paragraphs_from_lines` emits
/// them. Recomputed rather than threaded through, so that function keeps its
/// single job.
fn column_line_tops(runs: &[Placed], columns: &[Vec<Vec<usize>>]) -> Vec<f32> {
    let mut tops = Vec::new();
    for column in columns {
        let mut previous: Option<(f32, f32, f32)> = None;
        for line in column {
            if line.is_empty() {
                continue;
            }
            let top = line.iter().fold(f32::MAX, |v, i| v.min(runs[*i].piece.top));
            let bottom = line.iter().fold(0.0_f32, |v, i| v.max(runs[*i].piece.bottom));
            let left = line.iter().fold(f32::MAX, |v, i| v.min(runs[*i].piece.left));
            let height = (bottom - top).abs().max(1.0);
            let starts = match previous {
                None => true,
                Some((prev_bottom, prev_height, prev_left)) => {
                    (top - prev_bottom) > prev_height * PARAGRAPH_GAP
                        || (left - prev_left).abs() > prev_height
                }
            };
            if starts {
                tops.push(top);
            }
            previous = Some((bottom, height, left));
        }
    }
    tops
}

/// SPEC: P7-OCR-004 — put each run back in the cell it was drawn in.
fn fill_table(table: &Table, runs: &[&Placed]) -> FilledTable {
    let columns = table.column_count();
    let mut rows: Vec<Vec<Vec<Paragraph>>> =
        vec![vec![Vec::new(); columns]; table.row_count()];

    for cell in &table.cells {
        let inside: Vec<Placed> = runs
            .iter()
            .filter(|placed| in_cell(cell, &placed.piece))
            .map(|placed| (*placed).clone())
            .collect();
        if inside.is_empty() {
            continue;
        }
        // A cell's own runs go through the same ordering as anything else, so
        // a cell of two lines reads in the right order.
        let pieces: Vec<TextPiece> = inside.iter().map(|r| r.piece.clone()).collect();
        let grouped = group_pieces(&pieces);
        // Inside a cell there are no headings — a cell is not a document
        // section — so the body size is passed as zero, which disables them.
        let mut paragraphs = paragraphs_from_lines(&inside, &grouped, 0.0, &[]);
        // Trim the cell's edges. `PDFium` inserts a space between runs that sit
        // apart on a line, which across a cell boundary leaves "Region " rather
        // than "Region" — harmless in prose, wrong in a cell, where the text is
        // a value someone may sort or compare.
        for paragraph in &mut paragraphs {
            if let Some(first) = paragraph.runs.first_mut() {
                first.text = first.text.trim_start().to_owned();
            }
            if let Some(last) = paragraph.runs.last_mut() {
                last.text = last.text.trim_end().to_owned();
            }
        }
        paragraphs.retain(|p| p.runs.iter().any(|r| !r.text.is_empty()));
        if let Some(row) = rows.get_mut(cell.row) {
            if let Some(slot) = row.get_mut(cell.column) {
                *slot = paragraphs;
            }
        }
    }
    FilledTable { columns, rows }
}

/// Is this run drawn inside this cell? By its centre, so a glyph that overhangs
/// a rule by a hair still belongs to the cell it was written in.
fn in_cell(cell: &Cell, piece: &TextPiece) -> bool {
    let x = (piece.left + piece.right) / 2.0;
    let y = (piece.top + piece.bottom) / 2.0;
    x >= cell.left && x <= cell.right && y >= cell.top && y <= cell.bottom
}

fn paragraphs_from_lines(
    runs: &[Placed],
    columns: &[Vec<Vec<usize>>],
    body: f32,
    sizes: &[i32],
) -> Vec<Paragraph> {
    let mut out: Vec<Paragraph> = Vec::new();
    for column in columns {
        let column_width = column
            .iter()
            .flatten()
            .fold(0.0_f32, |w, i| w.max(runs[*i].piece.right))
            - column
                .iter()
                .flatten()
                .fold(f32::MAX, |w, i| w.min(runs[*i].piece.left));

        let mut previous: Option<(f32, f32, f32, Option<u8>)> = None; // bottom, height, left, heading
        for line in column {
            if line.is_empty() {
                continue;
            }
            let top = line.iter().fold(f32::MAX, |v, i| v.min(runs[*i].piece.top));
            let bottom = line.iter().fold(0.0_f32, |v, i| v.max(runs[*i].piece.bottom));
            let left = line.iter().fold(f32::MAX, |v, i| v.min(runs[*i].piece.left));
            let right = line.iter().fold(0.0_f32, |v, i| v.max(runs[*i].piece.right));
            let size = line.iter().fold(0.0_f32, |v, i| v.max(runs[*i].size));
            let height = (bottom - top).abs().max(1.0);
            let heading = heading_level(size, right - left, column_width, body, sizes);

            let starts_paragraph = match previous {
                None => true,
                Some((prev_bottom, prev_height, prev_left, prev_heading)) => {
                    heading != prev_heading
                        || (top - prev_bottom) > prev_height * PARAGRAPH_GAP
                        || (left - prev_left).abs() > prev_height
                }
            };

            let line_runs: Vec<StyledRun> = line
                .iter()
                .map(|i| StyledRun {
                    text: runs[*i].piece.text.clone(),
                    bold: runs[*i].bold,
                    italic: runs[*i].italic,
                    size: runs[*i].size,
                })
                .collect();

            if starts_paragraph {
                out.push(Paragraph { runs: line_runs, heading });
            } else if let Some(last) = out.last_mut() {
                // Lines of one paragraph are separate runs in the PDF but one
                // flow of text in Word; a space carries the line break.
                if let Some(tail) = last.runs.last_mut() {
                    if !tail.text.ends_with(' ') {
                        tail.text.push(' ');
                    }
                }
                last.runs.extend(line_runs);
            }
            previous = Some((bottom, height, left, heading));
        }
        previous = None;
        let _ = previous;
    }
    // Merge adjacent runs that look identical, so Word gets "one bold phrase"
    // rather than one run per glyph cluster the PDF happened to split at.
    for paragraph in &mut out {
        let mut merged: Vec<StyledRun> = Vec::with_capacity(paragraph.runs.len());
        for run in paragraph.runs.drain(..) {
            match merged.last_mut() {
                Some(last)
                    if last.bold == run.bold
                        && last.italic == run.italic
                        && bucket(last.size) == bucket(run.size) =>
                {
                    if !last.text.ends_with(' ') && !run.text.starts_with(' ') {
                        last.text.push(' ');
                    }
                    last.text.push_str(&run.text);
                }
                _ => merged.push(run),
            }
        }
        paragraph.runs = merged;
    }
    out
}

/// SPEC: P7-OCR-004 — convert `pages` (0-based, empty meaning all of them) to a
/// `.docx` and write it to `dest`.
///
/// The PDF is only read; nothing here modifies the document.
///
/// # Errors
/// When a page cannot be read, the archive cannot be built, or the write fails.
pub fn export_docx(
    doc: &PdfDocument<'_>,
    pages: &[usize],
    dest: &std::path::Path,
) -> Result<DocxExportSummary, CommandError> {
    let wanted: Vec<i32> = {
        let _guard = crate::pdf::document::pdfium_lock()?;
        let count = doc.pages().len();
        if pages.is_empty() {
            (0..count).collect()
        } else {
            pages
                .iter()
                .map(|p| {
                    i32::try_from(*p)
                        .ok()
                        .filter(|index| *index < count)
                        .ok_or_else(|| CommandError::InvalidInput(format!("page out of range: {p}")))
                })
                .collect::<Result<_, _>>()?
        }
    };

    // Two passes: the body size is a property of the whole document, so every
    // page has to be read before any paragraph can be told from any heading.
    let mut per_page = Vec::with_capacity(wanted.len());
    {
        let _guard = crate::pdf::document::pdfium_lock()?;
        for index in &wanted {
            per_page.push(page_content(doc, *index)?);
        }
    }

    let all_runs: Vec<StyledRun> = per_page
        .iter()
        .flat_map(|(runs, _, _)| {
            runs.iter().map(|r| StyledRun {
                text: r.piece.text.clone(),
                bold: r.bold,
                italic: r.italic,
                size: r.size,
            })
        })
        .collect();
    let body = body_size(&all_runs).unwrap_or(0.0);
    let sizes = heading_sizes(&all_runs, body);

    let mut blocks = Vec::new();
    let mut summary = DocxExportSummary {
        pages: u32::try_from(wanted.len()).unwrap_or(u32::MAX),
        ..DocxExportSummary::default()
    };
    for (runs, images, rules) in per_page {
        let tables = detect_tables(&rules);

        // A cell's text must not also be a paragraph, or the document says
        // everything twice. The split happens here, before any ordering, so
        // neither side ever sees the other's runs.
        let (in_table, outside): (Vec<&Placed>, Vec<&Placed>) = runs.iter().partition(|placed| {
            let x = (placed.piece.left + placed.piece.right) / 2.0;
            let y = (placed.piece.top + placed.piece.bottom) / 2.0;
            tables.iter().any(|table| table.contains(x, y))
        });

        let outside: Vec<Placed> = outside.into_iter().cloned().collect();
        let pieces: Vec<TextPiece> = outside.iter().map(|r| r.piece.clone()).collect();
        let columns = group_pieces(&pieces);
        let positioned = paragraphs_with_position(&outside, &columns, body, &sizes);

        // A table is inserted *into* the paragraph sequence, immediately before
        // the first paragraph that starts below it. The sequence itself is
        // never re-sorted: it is already in reading order, which on a
        // two-column page is not the same as top-to-bottom order. Sorting every
        // block by its top edge — which this did at first — interleaves the
        // columns again and undoes `group_pieces` entirely.
        let mut pending: Vec<&Table> = tables.iter().collect();
        pending.sort_by(|a, b| a.top().total_cmp(&b.top()));
        let mut pending = pending.into_iter().peekable();

        for (top, paragraph) in positioned {
            while pending.peek().is_some_and(|table| table.top() <= top) {
                let table = pending.next().unwrap_or_else(|| unreachable!("peeked"));
                summary.tables += 1;
                blocks.push(Block::Table(fill_table(table, &in_table)));
            }
            if paragraph.heading.is_some() {
                summary.headings += 1;
            }
            summary.paragraphs += 1;
            blocks.push(Block::Paragraph(paragraph));
        }
        for table in pending {
            summary.tables += 1;
            blocks.push(Block::Table(fill_table(table, &in_table)));
        }

        for image in images {
            summary.images += 1;
            blocks.push(Block::Image(image));
        }
    }

    let bytes = build_docx(&blocks)?;
    summary.bytes = bytes.len() as u64;
    std::fs::write(dest, &bytes)?;
    Ok(summary)
}

#[allow(clippy::expect_used, clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, size: f32) -> StyledRun {
        StyledRun { text: text.into(), bold: false, italic: false, size }
    }

    #[test]
    fn the_body_size_is_the_size_most_characters_are_set_in() {
        // Counted by characters, not by runs: a page of body text is a few long
        // runs and a title is one short one, so counting runs would let a
        // three-word heading outvote a paragraph.
        let runs = vec![
            run("Quarterly Report", 20.0),
            run("Revenue and Costs", 14.0),
            run("The quarter closed ahead of plan and margin held.", 9.0),
            run("Supply remains the main risk for the coming year.", 9.0),
        ];
        assert_eq!(body_size(&runs), Some(9.0));
    }

    #[test]
    fn a_tie_on_size_resolves_to_the_smaller() {
        // Body text is more common than display type; guessing large would turn
        // a whole document into headings.
        let runs = vec![run("aaaa", 18.0), run("bbbb", 10.0)];
        assert_eq!(body_size(&runs), Some(10.0));
    }

    #[test]
    fn an_empty_document_has_no_body_size() {
        assert_eq!(body_size(&[]), None);
        assert_eq!(body_size(&[run("   ", 12.0)]), None);
    }

    #[test]
    fn heading_sizes_are_distinct_and_largest_first() {
        let runs = vec![
            run("Title", 20.0),
            run("Section", 14.0),
            run("Another section", 14.0),
            run("body text here", 9.0),
        ];
        assert_eq!(heading_sizes(&runs, 9.0), vec![200, 140]);
    }

    #[test]
    fn heading_levels_follow_size_rank() {
        let sizes = vec![200, 140, 110];
        let level = |size: f32| heading_level(size, 40.0, 400.0, 9.0, &sizes);
        assert_eq!(level(20.0), Some(1));
        assert_eq!(level(14.0), Some(2));
        assert_eq!(level(11.0), Some(3));
        // Body text is never a heading.
        assert_eq!(level(9.0), None);
    }

    #[test]
    fn a_fourth_heading_size_does_not_invent_a_fourth_level() {
        // Word's outline has three levels here; a fourth rank folds into the
        // third rather than emitting a style the document does not define.
        let sizes = vec![200, 160, 140, 110];
        assert_eq!(heading_level(11.0, 40.0, 400.0, 9.0, &sizes), Some(3));
    }

    #[test]
    fn a_long_line_in_a_slightly_larger_face_is_not_a_heading() {
        // The width rule: a lead paragraph or pull-quote is bigger than body
        // text but fills its column. 10.5 pt over 9 pt body is 1.17x — larger,
        // but not obviously so.
        let sizes = vec![105];
        assert_eq!(heading_level(10.5, 390.0, 400.0, 9.0, &sizes), None);
        // The same size, short, is a heading.
        assert_eq!(heading_level(10.5, 60.0, 400.0, 9.0, &sizes), Some(1));
    }

    #[test]
    fn obviously_large_type_is_a_heading_however_long_the_line() {
        // Measured 2026-09-23: a plain 20 pt title over 9 pt body ran to 63% of
        // its column and the first width threshold rejected it. Past 1.5x the
        // body size, length stops being evidence.
        let sizes = vec![200];
        assert_eq!(heading_level(20.0, 390.0, 400.0, 9.0, &sizes), Some(1));
    }

    #[test]
    fn a_document_with_no_body_size_has_no_headings() {
        assert_eq!(heading_level(20.0, 40.0, 400.0, 0.0, &[200]), None);
    }

    #[test]
    fn points_convert_to_emu_exactly() {
        // 1 pt = 12,700 EMU, by definition.
        assert_eq!(points_to_emu(1.0), 12_700);
        assert_eq!(points_to_emu(72.0), 914_400); // one inch
        assert_eq!(points_to_emu(128.0), 1_625_600);
        // Never zero: a zero extent makes Word drop the image silently.
        assert_eq!(points_to_emu(0.0), 1);
        assert_eq!(points_to_emu(-5.0), 1);
    }

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
    fn a_paragraph_carries_its_runs_and_style() {
        let paragraph = Paragraph {
            runs: vec![
                StyledRun { text: "plain ".into(), bold: false, italic: false, size: 9.0 },
                StyledRun { text: "bold".into(), bold: true, italic: false, size: 9.0 },
            ],
            heading: Some(2),
        };
        let xml = paragraph_xml(&paragraph);
        assert!(xml.contains(r#"<w:pStyle w:val="Heading2"/>"#));
        assert!(xml.contains("<w:b/>"));
        // The plain run has no properties element at all.
        assert_eq!(xml.matches("<w:rPr>").count(), 1);
        // Spaces between runs must survive, or words run together in Word.
        assert!(xml.contains(r#"<w:t xml:space="preserve">plain </w:t>"#));
    }
}
