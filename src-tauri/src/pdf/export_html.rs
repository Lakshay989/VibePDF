//! SPEC: P7-OCR-007 — export a document as semantic HTML, with its images
//! extracted to a sibling folder.
//!
//! **Semantic, not a facsimile.** The output is the document's structure —
//! headings, paragraphs, emphasis, tables, images, in reading order — not its
//! page layout. A reader can reflow it, a screen reader can navigate it, and
//! another tool can parse it. Absolutely positioned boxes would reproduce the
//! look of each page and none of those.
//!
//! **The structure is not reimplemented.** It comes from
//! [`crate::pdf::export_docx::document_blocks`], the same blocks the Word
//! export writes, so the two exports cannot disagree about reading order,
//! headings or tables.
//!
//! **Self-contained and offline.** The page loads nothing from anywhere: no
//! fonts, no scripts, no stylesheet links. Its few lines of CSS are inline, and
//! its images are files beside it, linked by relative path.

use std::fmt::Write as _;
use std::path::Path;

use pdfium_render::prelude::PdfDocument;
use serde::Serialize;

use crate::error::CommandError;
use crate::pdf::document::collect_metadata;
use crate::pdf::export_docx::{document_blocks, Block, FilledTable, Paragraph};
use crate::pdf::ooxml::escape_xml;
use crate::pdf::page_content::EmbeddedImage;

/// What an export produced.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HtmlExportSummary {
    pub pages: u32,
    pub paragraphs: u32,
    pub headings: u32,
    pub images: u32,
    pub tables: u32,
    /// Size of the `.html` file alone.
    pub bytes: u64,
    /// The folder the images went to, beside the `.html`; `None` when the
    /// document had no images and no folder was created.
    pub images_folder: Option<String>,
}

/// Just enough to read comfortably and to show a table as one. Nothing here
/// positions anything, and nothing is fetched.
const STYLE: &str = "body{max-width:46rem;margin:2rem auto;padding:0 1rem;\
line-height:1.5;font-family:system-ui,sans-serif}\
img{max-width:100%;height:auto}\
table{border-collapse:collapse;margin:1rem 0}\
td{border:1px solid #999;padding:.25rem .5rem;vertical-align:top}\
td p{margin:0}";

/// CSS pixels per PDF point: a point is 1/72 in, a CSS pixel 1/96 in.
const PX_PER_POINT: f32 = 96.0 / 72.0;

/// `report_files` for `report.html` — the name browsers give the folder when
/// they save a page complete, so it reads as belonging to the file beside it.
#[must_use]
pub fn images_folder_name(dest: &Path) -> String {
    let stem = dest.file_stem().map_or_else(|| "document".into(), |s| s.to_string_lossy().into_owned());
    format!("{stem}_files")
}

/// `segment` as one path segment of a relative URL: RFC 3986 unreserved
/// characters as they are, every other byte of its UTF-8 percent-encoded.
///
/// Without this a file saved as `My report.html` links its images as
/// `My report_files/…`, and a `#` or `%` in the name breaks the link outright.
fn encode_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// The file name of the `index`-th image (0-based) inside the images folder.
fn image_file_name(index: usize) -> String {
    format!("image-{:03}.png", index + 1)
}

fn paragraph_html(paragraph: &Paragraph) -> String {
    let tag = match paragraph.heading {
        Some(level @ 1..=3) => format!("h{level}"),
        _ => "p".to_string(),
    };
    let mut out = format!("<{tag}>");
    for run in &paragraph.runs {
        let text = escape_xml(&run.text);
        match (run.bold, run.italic) {
            (true, true) => {
                let _ = write!(out, "<strong><em>{text}</em></strong>");
            }
            (true, false) => {
                let _ = write!(out, "<strong>{text}</strong>");
            }
            (false, true) => {
                let _ = write!(out, "<em>{text}</em>");
            }
            (false, false) => out.push_str(&text),
        }
    }
    let _ = write!(out, "</{tag}>");
    out
}

fn table_html(table: &FilledTable) -> String {
    let mut out = String::from("<table><tbody>");
    for row in &table.rows {
        out.push_str("<tr>");
        for cell in row {
            out.push_str("<td>");
            for paragraph in cell {
                out.push_str(&paragraph_html(paragraph));
            }
            out.push_str("</td>");
        }
        out.push_str("</tr>");
    }
    out.push_str("</tbody></table>");
    out
}

/// One image, sized as it was drawn on the page. `alt` is empty — marking it
/// decorative — because nothing here knows what the image shows, and a made-up
/// description is worse for a screen-reader user than none.
fn image_html(src: &str, image: &EmbeddedImage) -> String {
    // Rounded and clamped: a drawn size is a positive number of points, and a
    // zero-sized <img> would hide the picture.
    let px = |points: f32| format!("{:.0}", (points * PX_PER_POINT).max(1.0));
    format!(
        r#"<p><img src="{src}" width="{}" height="{}" alt=""></p>"#,
        px(image.width_points),
        px(image.height_points)
    )
}

/// SPEC: P7-OCR-007 — the document as one HTML page. Images are referenced as
/// `<images_folder>/image-NNN.png`, numbered in the order they appear.
#[must_use]
pub fn build_html(title: &str, blocks: &[Block], images_folder: &str) -> String {
    let folder = encode_segment(images_folder);
    let mut body = String::new();
    let mut image_index = 0;
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => body.push_str(&paragraph_html(paragraph)),
            Block::Table(table) => body.push_str(&table_html(table)),
            Block::Image(image) => {
                let src = format!("{folder}/{}", image_file_name(image_index));
                image_index += 1;
                body.push_str(&image_html(&src, image));
            }
        }
        body.push('\n');
    }
    format!(
        "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <meta name=\"generator\" content=\"VibePDF\">\n<title>{}</title>\n<style>{STYLE}</style>\n\
         </head>\n<body>\n{body}</body>\n</html>\n",
        escape_xml(title)
    )
}

/// SPEC: P7-OCR-007 — convert `pages` (0-based, empty meaning all of them) to
/// HTML at `dest`, writing any images to `<stem>_files/` beside it.
///
/// The PDF is only read. Inside the images folder only `image-NNN.png` files
/// are written; an existing folder of that name keeps everything else in it.
///
/// # Errors
/// When a page cannot be read, or a file or the folder cannot be written.
pub fn export_html(
    doc: &PdfDocument<'_>,
    pages: &[usize],
    dest: &Path,
) -> Result<HtmlExportSummary, CommandError> {
    let (blocks, counts) = document_blocks(doc, pages)?;

    let title = collect_metadata(doc)
        .title
        .filter(|t| !t.trim().is_empty())
        .or_else(|| dest.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "Document".into());
    let folder_name = images_folder_name(dest);

    let images: Vec<&EmbeddedImage> = blocks
        .iter()
        .filter_map(|b| match b {
            Block::Image(image) => Some(image),
            Block::Paragraph(_) | Block::Table(_) => None,
        })
        .collect();
    if !images.is_empty() {
        let folder = dest.with_file_name(&folder_name);
        std::fs::create_dir_all(&folder)?;
        for (index, image) in images.iter().enumerate() {
            std::fs::write(folder.join(image_file_name(index)), &image.png)?;
        }
    }

    let html = build_html(&title, &blocks, &folder_name);
    std::fs::write(dest, html.as_bytes())?;

    Ok(HtmlExportSummary {
        pages: counts.pages,
        paragraphs: counts.paragraphs,
        headings: counts.headings,
        images: counts.images,
        tables: counts.tables,
        bytes: html.len() as u64,
        images_folder: (!images.is_empty()).then_some(folder_name),
    })
}

#[cfg(test)]
mod tests {
    use super::{build_html, encode_segment, images_folder_name};
    use crate::pdf::export_docx::{Block, FilledTable, Paragraph, StyledRun};
    use crate::pdf::page_content::EmbeddedImage;
    use std::path::Path;

    fn run(text: &str, bold: bool, italic: bool) -> StyledRun {
        StyledRun { text: text.into(), bold, italic, size: 10.0 }
    }

    fn para(runs: Vec<StyledRun>, heading: Option<u8>) -> Block {
        Block::Paragraph(Paragraph { runs, heading })
    }

    #[test]
    fn a_path_segment_keeps_unreserved_characters_and_encodes_the_rest() {
        assert_eq!(encode_segment("report_files"), "report_files");
        assert_eq!(encode_segment("My report_files"), "My%20report_files");
        assert_eq!(encode_segment("a#b%c?d"), "a%23b%25c%3Fd");
        assert_eq!(encode_segment("Bericht-ü"), "Bericht-%C3%BC");
    }

    #[test]
    fn the_folder_is_named_after_the_html_file() {
        assert_eq!(images_folder_name(Path::new("/tmp/out/Q3 report.html")), "Q3 report_files");
    }

    // Text from a PDF is untrusted: a run that spells out markup must arrive
    // as text, never as elements.
    #[test]
    fn text_that_looks_like_markup_stays_text() {
        let html = build_html(
            "<title>",
            &[para(vec![run("<script>alert(1)</script> & \"x\"", false, false)], None)],
            "f",
        );
        assert!(!html.contains("<script>"), "{html}");
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt; &amp; &quot;x&quot;"), "{html}");
        assert!(html.contains("<title>&lt;title&gt;</title>"), "{html}");
    }

    #[test]
    fn headings_and_emphasis_become_their_elements() {
        let html = build_html(
            "t",
            &[
                para(vec![run("Title", true, false)], Some(1)),
                para(vec![run("Part", false, false)], Some(3)),
                para(vec![run("plain ", false, false), run("bold", true, false), run(" both", true, true)], None),
                para(vec![run("lean", false, true)], None),
            ],
            "f",
        );
        assert!(html.contains("<h1><strong>Title</strong></h1>"), "{html}");
        assert!(html.contains("<h3>Part</h3>"), "{html}");
        assert!(html.contains("<p>plain <strong>bold</strong><strong><em> both</em></strong></p>"), "{html}");
        assert!(html.contains("<p><em>lean</em></p>"), "{html}");
    }

    #[test]
    fn images_are_numbered_in_order_and_linked_through_the_encoded_folder() {
        let image = |w: f32| Block::Image(EmbeddedImage { png: vec![], width_points: w, height_points: 72.0 });
        let html = build_html("t", &[image(72.0), para(vec![run("x", false, false)], None), image(144.0)], "My report_files");
        assert!(html.contains(r#"<img src="My%20report_files/image-001.png" width="96" height="96" alt="">"#), "{html}");
        assert!(html.contains(r#"<img src="My%20report_files/image-002.png" width="192" height="96" alt="">"#), "{html}");
    }

    #[test]
    fn a_table_keeps_its_rows_and_cells() {
        let cell = |t: &str| vec![Paragraph { runs: vec![run(t, false, false)], heading: None }];
        let table = FilledTable { columns: 2, rows: vec![vec![cell("a"), cell("b")], vec![cell("c"), vec![]]] };
        let html = build_html("t", &[Block::Table(table)], "f");
        assert!(
            html.contains("<table><tbody><tr><td><p>a</p></td><td><p>b</p></td></tr><tr><td><p>c</p></td><td></td></tr></tbody></table>"),
            "{html}"
        );
    }

    #[test]
    fn the_page_loads_nothing_from_anywhere() {
        let html = build_html("t", &[para(vec![run("x", false, false)], None)], "f");
        for needle in ["http:", "https:", "//", "<link", "<script", "@import", "url("] {
            assert!(!html.contains(needle), "found {needle:?} in {html}");
        }
    }
}
