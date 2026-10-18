//! SPEC: P7-OCR-009 (P7.C1b1) — create a PDF from a Word document.
//!
//! Three kinds of input. Hand-built `.docx` files, one feature each, so a
//! failure names the feature. `textutil-memo.docx`, written by macOS's own
//! `textutil` — a writer we didn't make, with its own habits (direct
//! formatting, literal list text, the pre-standard `w:first-line`). And a
//! round trip through VibePDF's own Word export.
//!
//! What the tests trust: PDFium (and Ghostscript, where it's installed)
//! extract the text, and it must equal the document's; PDFium renders pages
//! for pixel checks; lopdf lists the fonts, whose names are the faces' own
//! PostScript names, so "bold was drawn in the bold face" is checkable.

use std::path::{Path, PathBuf};
use std::process::Command;

use lopdf::{Document, Object};
use vibepdf_lib::error::CommandError;
use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::create::{create_pdf_from_files, CreateSummary, PageSize};
use vibepdf_lib::pdf::document::open_pdf;
use vibepdf_lib::pdf::ooxml::{read_entry, ZipWriter};
use vibepdf_lib::pdf::render::{render_page, ImageFormat};
use vibepdf_lib::pdf::text_extract::{extract_text_runs, TextRun};

const NAMESPACES: &str = concat!(
    r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" "#,
    r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" "#,
    r#"xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" "#,
    r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" "#,
    r#"xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture""#
);
const LETTER: &str = r#"<w:sectPr><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr>"#;

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("vibepdf-docx-in-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Self(dir)
    }
    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, bytes).expect("write");
        path
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A `.docx` whose body is `body`, with optional styles and numbering parts
/// and `media` as `word/media/<name>` behind relationship `rId<name>`.
fn docx(body: &str, styles: Option<&str>, numbering: Option<&str>, media: &[(&str, &[u8])]) -> Vec<u8> {
    let mut rels = String::from(r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#);
    let kind = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    if styles.is_some() {
        rels.push_str(&format!(r#"<Relationship Id="rIdStyles" Type="{kind}/styles" Target="styles.xml"/>"#));
    }
    if numbering.is_some() {
        rels.push_str(&format!(r#"<Relationship Id="rIdNumbering" Type="{kind}/numbering" Target="numbering.xml"/>"#));
    }
    for (name, _) in media {
        rels.push_str(&format!(r#"<Relationship Id="rId{name}" Type="{kind}/image" Target="media/{name}"/>"#));
    }
    rels.push_str("</Relationships>");

    let mut zip = ZipWriter::new();
    let add = |zip: &mut ZipWriter, name: &str, data: &[u8]| zip.add(name, data, true).expect("add");
    add(&mut zip, "[Content_Types].xml", br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#);
    add(&mut zip, "_rels/.rels", br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#);
    add(&mut zip, "word/document.xml", format!(r#"<?xml version="1.0"?><w:document {NAMESPACES}><w:body>{body}</w:body></w:document>"#).as_bytes());
    add(&mut zip, "word/_rels/document.xml.rels", rels.as_bytes());
    if let Some(styles) = styles {
        add(&mut zip, "word/styles.xml", format!(r#"<?xml version="1.0"?><w:styles {NAMESPACES}>{styles}</w:styles>"#).as_bytes());
    }
    if let Some(numbering) = numbering {
        add(&mut zip, "word/numbering.xml", format!(r#"<?xml version="1.0"?><w:numbering {NAMESPACES}>{numbering}</w:numbering>"#).as_bytes());
    }
    for (name, bytes) in media {
        zip.add(&format!("word/media/{name}"), bytes, false).expect("media");
    }
    zip.finish().expect("zip")
}

/// One paragraph of one run in `font`.
fn para(text: &str, rpr: &str) -> String {
    format!(r#"<w:p><w:r><w:rPr>{rpr}</w:rPr><w:t xml:space="preserve">{text}</w:t></w:r></w:p>"#)
}

fn arial() -> &'static str {
    r#"<w:rFonts w:ascii="Arial" w:hAnsi="Arial"/><w:sz w:val="22"/>"#
}

fn convert(scratch: &Scratch, docx_bytes: &[u8]) -> (PathBuf, CreateSummary) {
    let input = scratch.write("in.docx", docx_bytes);
    let dest = scratch.path("out.pdf");
    let summary = create_pdf_from_files(&[input], &dest, PageSize::A4).expect("create succeeds");
    (dest, summary)
}

fn runs(path: &Path) -> Vec<Vec<TextRun>> {
    let (doc, meta) = open_pdf(path, None).expect("opens");
    (0..usize::try_from(meta.page_count).expect("count")).map(|p| extract_text_runs(&doc, p).expect("extract")).collect()
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The pages' text as a reader would take it: runs that touch on the same
/// baseline joined directly (a bold word and the comma after it are two
/// runs), anything else separated by a space.
fn text(path: &Path) -> String {
    let mut out = String::new();
    let mut previous: Option<TextRun> = None;
    for run in runs(path).into_iter().flatten() {
        if let Some(prev) = &previous {
            let same_line = (prev.bbox[1] - run.bbox[1]).abs() < 1.0;
            let touching = (run.bbox[0] - prev.bbox[2]).abs() < 0.2 * run.font_size.max(1.0);
            if !(same_line && touching) {
                out.push(' ');
            }
        }
        out.push_str(&run.text);
        previous = Some(run);
    }
    collapse(&out)
}

/// The run whose text contains `needle`'s first word. PDFium may split a
/// line into runs anywhere the font or position changes, so a phrase is
/// found by where it starts.
fn run_with(path: &Path, needle: &str) -> TextRun {
    let first = needle.split_whitespace().next().unwrap_or(needle);
    runs(path).into_iter().flatten().find(|r| r.text.contains(first)).unwrap_or_else(|| panic!("no run with {first:?}"))
}

/// Every embedded font's name, subset tag removed.
fn font_names(path: &Path) -> Vec<String> {
    let doc = Document::load_mem(&std::fs::read(path).expect("read")).expect("parse");
    let mut names: Vec<String> = doc
        .objects
        .values()
        .filter_map(|o| o.as_dict().ok())
        .filter(|d| d.get(b"Type").and_then(Object::as_name).is_ok_and(|t| t == b"FontDescriptor"))
        .filter_map(|d| d.get(b"FontName").and_then(Object::as_name).ok())
        .map(|n| String::from_utf8_lossy(n).split_once('+').map_or_else(String::new, |(_, n)| n.to_owned()))
        .collect();
    names.sort();
    names.dedup();
    names
}

/// The text of a `.docx`'s `<w:t>` elements, read crudely and independently
/// of the reader under test: a paragraph's pieces joined as written (Word
/// splits runs mid-word), paragraphs and tabs as spaces.
fn docx_text(bytes: &[u8]) -> String {
    let xml = String::from_utf8(read_entry(bytes, "word/document.xml").expect("document.xml")).expect("utf-8");
    let mut out = String::new();
    for paragraph in xml.split("</w:p>") {
        for piece in paragraph.split('<').skip(1) {
            if piece.starts_with("w:tab/") || piece.starts_with("w:tab ") {
                out.push(' ');
            } else if let Some(rest) = piece.strip_prefix("w:t>").or_else(|| piece.strip_prefix("w:t ").and_then(|p| p.split_once('>').map(|(_, r)| r))) {
                out.push_str(rest);
            }
        }
        out.push(' ');
    }
    let decoded = out.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&");
    collapse(&decoded)
}

fn ghostscript() -> Option<&'static str> {
    ["/opt/homebrew/bin/gs", "/usr/local/bin/gs", "gs"]
        .into_iter()
        .find(|p| Command::new(p).arg("--version").output().is_ok_and(|o| o.status.success()))
}

// --- text and formatting ------------------------------------------------------

#[test]
fn paragraphs_come_out_in_order_with_their_text() {
    let scratch = Scratch::new("order");
    let body = [para("First paragraph.", arial()), para("Second — naïve café, Счёт.", arial()), LETTER.into()].concat();
    let (pdf, summary) = convert(&scratch, &docx(&body, None, None, &[]));
    assert_eq!((summary.documents, summary.pages), (1, 1));
    assert_eq!(text(&pdf), "First paragraph. Second — naïve café, Счёт.");
}

#[test]
fn bold_and_italic_are_drawn_in_the_real_faces() {
    let scratch = Scratch::new("faces");
    let font = r#"<w:rFonts w:ascii="Arial" w:hAnsi="Arial"/>"#;
    let body = format!(
        r#"<w:p><w:r><w:rPr>{font}</w:rPr><w:t xml:space="preserve">plain </w:t></w:r><w:r><w:rPr>{font}<w:b/></w:rPr><w:t xml:space="preserve">bold </w:t></w:r><w:r><w:rPr>{font}<w:i/></w:rPr><w:t xml:space="preserve">italic </w:t></w:r><w:r><w:rPr>{font}<w:b/><w:i/></w:rPr><w:t>both</w:t></w:r></w:p>{LETTER}"#
    );
    let (pdf, _) = convert(&scratch, &docx(&body, None, None, &[]));
    assert_eq!(font_names(&pdf), vec!["Arial-BoldItalicMT", "Arial-BoldMT", "Arial-ItalicMT", "ArialMT"]);
    assert_eq!(text(&pdf), "plain bold italic both");
}

#[test]
fn styles_apply_through_their_chain() {
    let scratch = Scratch::new("styles");
    let styles = r#"
        <w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Arial" w:hAnsi="Arial"/><w:sz w:val="22"/></w:rPr></w:rPrDefault></w:docDefaults>
        <w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>
        <w:style w:type="paragraph" w:styleId="Heading1"><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:before="240" w:after="120"/></w:pPr><w:rPr><w:b/><w:sz w:val="32"/></w:rPr></w:style>
        <w:style w:type="paragraph" w:styleId="Subtle"><w:basedOn w:val="Heading1"/><w:rPr><w:i/><w:color w:val="1F4E79"/></w:rPr></w:style>"#;
    let body = format!(
        r#"<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Heading text</w:t></w:r></w:p>
           <w:p><w:r><w:t>Body text</w:t></w:r></w:p>
           <w:p><w:pPr><w:pStyle w:val="Subtle"/></w:pPr><w:r><w:t>Inherited</w:t></w:r></w:p>{LETTER}"#
    );
    let (pdf, _) = convert(&scratch, &docx(&body, Some(styles), None, &[]));
    let heading = run_with(&pdf, "Heading text");
    let body_run = run_with(&pdf, "Body text");
    assert!((heading.font_size - 16.0).abs() < 0.5 && (body_run.font_size - 11.0).abs() < 0.5, "{} / {}", heading.font_size, body_run.font_size);
    assert!(heading.font_name.contains("Bold"), "heading font {}", heading.font_name);
    // Subtle is based on Heading1: bold and 16 pt from it, plus its own italic.
    let inherited = run_with(&pdf, "Inherited");
    assert!(inherited.font_name.contains("BoldItalic"), "{}", inherited.font_name);
    assert!((inherited.font_size - 16.0).abs() < 0.5);
}

#[test]
fn alignment_is_kept() {
    let scratch = Scratch::new("align");
    let jc = |v: &str, t: &str| format!(r#"<w:p><w:pPr><w:jc w:val="{v}"/></w:pPr><w:r><w:rPr>{}</w:rPr><w:t>{t}</w:t></w:r></w:p>"#, arial());
    let long = "Justified text keeps going across the line and then onto another line so that the first line has to be stretched to reach the right margin exactly as a justified paragraph does in Word.";
    let body = [jc("center", "Centred"), jc("right", "Righted"), jc("both", long), LETTER.into()].concat();
    let (pdf, _) = convert(&scratch, &docx(&body, None, None, &[]));
    let centred = run_with(&pdf, "Centred");
    assert!(((centred.bbox[0] + centred.bbox[2]) / 2.0 - 306.0).abs() < 2.0, "not centred: {:?}", centred.bbox);
    let righted = run_with(&pdf, "Righted");
    assert!((righted.bbox[2] - 540.0).abs() < 2.0, "not right-aligned: {:?}", righted.bbox);
    // The first justified line ends at the right margin; PDFium splits runs
    // at the stretched gaps, so look at the rightmost piece on that line.
    let all: Vec<TextRun> = runs(&pdf).into_iter().flatten().collect();
    let first_line_y = run_with(&pdf, "Justified").bbox[1];
    let right_edge = all.iter().filter(|r| (r.bbox[1] - first_line_y).abs() < 2.0).map(|r| r.bbox[2]).fold(0.0, f32::max);
    assert!((right_edge - 540.0).abs() < 2.0, "the justified line ends at {right_edge}");
}

#[test]
fn lists_get_their_bullets_and_numbers() {
    let scratch = Scratch::new("lists");
    let numbering = r#"
        <w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val="&#xF0B7;"/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr><w:rPr><w:rFonts w:ascii="Symbol" w:hAnsi="Symbol"/></w:rPr></w:lvl></w:abstractNum>
        <w:abstractNum w:abstractNumId="1">
          <w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl>
          <w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="lowerLetter"/><w:lvlText w:val="%2)"/><w:pPr><w:ind w:left="1440" w:hanging="360"/></w:pPr></w:lvl>
        </w:abstractNum>
        <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
        <w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num>"#;
    let item = |num: u32, level: u32, t: &str| {
        format!(r#"<w:p><w:pPr><w:numPr><w:ilvl w:val="{level}"/><w:numId w:val="{num}"/></w:numPr></w:pPr><w:r><w:rPr>{}</w:rPr><w:t>{t}</w:t></w:r></w:p>"#, arial())
    };
    let body = [item(1, 0, "Apples"), item(1, 0, "Pears"), item(2, 0, "First"), item(2, 0, "Second"), item(2, 1, "Sub"), item(2, 0, "Third"), LETTER.into()].concat();
    let (pdf, _) = convert(&scratch, &docx(&body, None, Some(numbering), &[]));
    assert_eq!(text(&pdf), "• Apples • Pears 1. First 2. Second a) Sub 3. Third");
    // The bullet hangs at 0.25 in from the margin; its text starts at 0.5 in.
    let apples = run_with(&pdf, "Apples");
    assert!((apples.bbox[0] - (72.0 + 36.0)).abs() < 1.5, "text not at the indent: {:?}", apples.bbox);
}

#[test]
fn page_size_and_margins_come_from_the_document() {
    let scratch = Scratch::new("page");
    let a4 = r#"<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="720" w:right="720" w:bottom="720" w:left="2160"/></w:sectPr>"#;
    let body = [para("Margin", arial()), a4.into()].concat();
    let (pdf, _) = convert(&scratch, &docx(&body, None, None, &[]));
    let doc = Document::load_mem(&std::fs::read(&pdf).expect("read")).expect("parse");
    let page = doc.get_pages().into_values().next().expect("a page");
    let media: Vec<f32> = doc
        .get_dictionary(page)
        .and_then(|d| d.get(b"MediaBox"))
        .and_then(Object::as_array)
        .expect("MediaBox")
        .iter()
        .map(|v| v.as_float().expect("number"))
        .collect();
    assert!((media[2] - 595.3).abs() < 0.05 && (media[3] - 841.9).abs() < 0.05, "not A4: {media:?}");
    let run = run_with(&pdf, "Margin");
    assert!((run.bbox[0] - 108.0).abs() < 1.5, "left margin not 1.5 in: {:?}", run.bbox);
    assert!(run.bbox[3] > 842.0 - 36.0 - 14.0, "top margin not 0.5 in: {:?}", run.bbox);
}

#[test]
fn a_page_break_starts_a_new_page() {
    let scratch = Scratch::new("break");
    let body = format!(
        r#"<w:p><w:r><w:rPr>{0}</w:rPr><w:t>before</w:t></w:r><w:r><w:br w:type="page"/></w:r><w:r><w:rPr>{0}</w:rPr><w:t>after</w:t></w:r></w:p>{LETTER}"#,
        arial()
    );
    let (pdf, summary) = convert(&scratch, &docx(&body, None, None, &[]));
    assert_eq!(summary.pages, 2);
    let pages = runs(&pdf);
    assert!(pages[0].iter().any(|r| r.text.contains("before")) && pages[1].iter().any(|r| r.text.contains("after")));
}

#[test]
fn long_documents_flow_onto_more_pages() {
    let scratch = Scratch::new("flow");
    let body: String = (1..=120).map(|n| para(&format!("Line number {n} of the document."), arial())).collect::<String>() + LETTER;
    let (pdf, summary) = convert(&scratch, &docx(&body, None, None, &[]));
    assert!(summary.pages >= 3, "120 lines in 11 pt on Letter is three pages, got {}", summary.pages);
    assert_eq!(text(&pdf), collapse(&(1..=120).map(|n| format!("Line number {n} of the document. ")).collect::<String>()));
}

#[test]
fn an_inline_image_is_drawn_at_its_size() {
    let scratch = Scratch::new("image");
    // 40 x 20 pixels, quadrants red / green over blue / white, shown 2 in x 1 in.
    let pixels: Vec<u8> = (0..20u32)
        .flat_map(|y| (0..40u32).map(move |x| match (x >= 20, y >= 10) {
            (false, false) => [220, 30, 30],
            (true, false) => [30, 180, 30],
            (false, true) => [30, 30, 220],
            (true, true) => [250, 250, 250],
        }))
        .flatten()
        .collect();
    let mut png_bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png_bytes, 40, 20);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().expect("header").write_image_data(&pixels).expect("data");
    }
    let drawing = r#"<w:p><w:r><w:drawing><wp:inline><wp:extent cx="1828800" cy="914400"/><a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="rIdpicture.png"/></pic:blipFill></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"#;
    let body = [drawing.to_owned(), LETTER.to_owned()].concat();
    let (pdf, _) = convert(&scratch, &docx(&body, None, None, &[("picture.png", &png_bytes)]));
    let (doc, _) = open_pdf(&pdf, None).expect("opens");
    let page = render_page(&doc, 0, 72.0, ImageFormat::Rgba8).expect("render");
    let at = |x: u32, y: u32| {
        let i = usize::try_from((y * page.width + x) * 4).expect("index");
        [page.bytes[i], page.bytes[i + 1], page.bytes[i + 2]]
    };
    let close = |a: [u8; 3], b: [u8; 3]| a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 40);
    // Top-left of the text area is (72, 72) from the page's top-left; the image is 144 x 72.
    assert!(close(at(108, 90), [220, 30, 30]), "top-left {:?}", at(108, 90));
    assert!(close(at(180, 90), [30, 180, 30]), "top-right {:?}", at(180, 90));
    assert!(close(at(108, 126), [30, 30, 220]), "bottom-left {:?}", at(108, 126));
    assert!(close(at(230, 90), [255, 255, 255]), "the image is wider than 2 in");
}

// --- what is simplified, and said ----------------------------------------------

#[test]
fn a_missing_font_is_substituted_and_reported() {
    let scratch = Scratch::new("missing");
    let body = [para("Substituted", r#"<w:rFonts w:ascii="Nonexistent Sans Test" w:hAnsi="Nonexistent Sans Test"/>"#), LETTER.into()].concat();
    let (pdf, summary) = convert(&scratch, &docx(&body, None, None, &[]));
    assert!(
        summary.warnings.iter().any(|w| w.starts_with("Nonexistent Sans Test isn't installed here")),
        "{:?}",
        summary.warnings
    );
    assert_eq!(text(&pdf), "Substituted");
}

#[test]
fn characters_the_face_lacks_come_from_the_broad_face() {
    let scratch = Scratch::new("fallback");
    let body = [para("Arial then 漢字", arial()), LETTER.into()].concat();
    let (pdf, summary) = convert(&scratch, &docx(&body, None, None, &[]));
    assert_eq!(text(&pdf), "Arial then 漢字");
    assert!(font_names(&pdf).len() >= 2, "one face drew everything: {:?}", font_names(&pdf));
    assert!(!summary.warnings.iter().any(|w| w.contains("shown as a box")), "{:?}", summary.warnings);
}

#[test]
fn tracked_changes_fields_and_hidden_text() {
    let scratch = Scratch::new("changes");
    let body = format!(
        r#"<w:p>
            <w:r><w:t xml:space="preserve">kept </w:t></w:r>
            <w:ins><w:r><w:t xml:space="preserve">inserted </w:t></w:r></w:ins>
            <w:del><w:r><w:delText>deleted </w:delText></w:r></w:del>
            <w:r><w:rPr><w:vanish/></w:rPr><w:t>hidden </w:t></w:r>
            <w:r><w:t xml:space="preserve">page </w:t></w:r>
            <w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> PAGE </w:instrText></w:r>
            <w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>7</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r>
           </w:p>{LETTER}"#
    );
    let (pdf, summary) = convert(&scratch, &docx(&body, None, None, &[]));
    assert_eq!(text(&pdf), "kept inserted page 7");
    assert!(summary.warnings.iter().any(|w| w.contains("Tracked changes are shown as if they had been accepted")), "{:?}", summary.warnings);
}

#[test]
fn a_table_keeps_its_text_and_says_it_was_simplified() {
    let scratch = Scratch::new("table");
    let cell = |t: &str| format!("<w:tc><w:p><w:r><w:t>{t}</w:t></w:r></w:p></w:tc>");
    let body = format!("<w:tbl><w:tr>{}{}</w:tr><w:tr>{}{}</w:tr></w:tbl>{LETTER}", cell("A1"), cell("B1"), cell("A2"), cell("B2"));
    let (pdf, summary) = convert(&scratch, &docx(&body, None, None, &[]));
    assert_eq!(text(&pdf), "A1 B1 A2 B2");
    assert!(summary.warnings.iter().any(|w| w.contains("Tables are shown as their cells")), "{:?}", summary.warnings);
}

#[test]
fn every_font_is_embedded() {
    let scratch = Scratch::new("embedded");
    let body = [para("Embedded", arial()), para("also", r#"<w:rFonts w:ascii="Times New Roman" w:hAnsi="Times New Roman"/><w:i/>"#), LETTER.into()].concat();
    let (pdf, _) = convert(&scratch, &docx(&body, None, None, &[]));
    let doc = Document::load_mem(&std::fs::read(&pdf).expect("read")).expect("parse");
    let descriptors: Vec<_> = doc
        .objects
        .values()
        .filter_map(|o| o.as_dict().ok())
        .filter(|d| d.get(b"Type").and_then(Object::as_name).is_ok_and(|t| t == b"FontDescriptor"))
        .collect();
    assert!(descriptors.len() >= 2);
    assert!(descriptors.iter().all(|d| d.has(b"FontFile2")), "a font is not embedded");
}

#[test]
fn a_damaged_document_is_refused_by_name() {
    let scratch = Scratch::new("damaged");
    let input = scratch.write("broken.docx", b"PK\x03\x04 then nothing a reader can use");
    let dest = scratch.path("out.pdf");
    match create_pdf_from_files(&[input], &dest, PageSize::A4) {
        Err(CommandError::InvalidInput(m)) => assert!(m.starts_with("broken.docx"), "{m}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(!dest.exists());
}

// --- documents we did not write ----------------------------------------------

#[test]
fn a_document_from_macos_textutil_keeps_its_text_and_faces() {
    let fixture = PathBuf::from("../tests/fixtures/basic/textutil-memo.docx");
    let bytes = std::fs::read(&fixture).expect("fixture");
    let scratch = Scratch::new("textutil");
    let (pdf, summary) = convert(&scratch, &bytes);
    assert_eq!(text(&pdf), docx_text(&bytes), "warnings: {:?}", summary.warnings);
    let fonts = font_names(&pdf);
    assert!(fonts.iter().any(|f| f.contains("Bold")), "the bold run wasn't bold: {fonts:?}");
    assert!(fonts.iter().any(|f| f.contains("Italic")), "the italic run wasn't italic: {fonts:?}");
    // textutil sizes its h1 at 24 pt over 12 pt body.
    assert!(run_with(&pdf, "Quarterly memo").font_size > run_with(&pdf, "Supply lead times").font_size * 1.5);
    let centred = run_with(&pdf, "Centred line");
    assert!(((centred.bbox[0] + centred.bbox[2]) / 2.0 - 306.0).abs() < 3.0, "{:?}", centred.bbox);
    // textutil writes a list item as "tab, bullet, tab, text" with the old
    // `w:first-line="-720"` under a 0.5 in left indent: the line starts at the
    // margin, the tabs stop at 0.5 in and 1 in, so the text starts 1 in in.
    let item = run_with(&pdf, "Supply lead times");
    assert!((item.bbox[0] - 144.0).abs() < 2.0, "list text at {:?}", item.bbox);
    assert!(!summary.warnings.iter().any(|w| w.contains("isn't installed")), "{:?}", summary.warnings);
    // Times covers everything here, Cyrillic included: no substitute and no
    // fallback face should appear.
    assert_eq!(fonts, vec!["Times-Bold", "Times-Italic", "Times-Roman"]);
}

// Didot has no Cyrillic. The fallback for it is another serif, not the sans
// broad-coverage face, so a line doesn't switch style in the middle.
#[cfg(target_os = "macos")]
#[test]
fn a_missing_script_falls_back_to_a_face_of_the_same_class() {
    let scratch = Scratch::new("serif-fallback");
    let body = [para("Didot and Счёт", r#"<w:rFonts w:ascii="Didot" w:hAnsi="Didot"/><w:sz w:val="24"/>"#), LETTER.into()].concat();
    let (pdf, _) = convert(&scratch, &docx(&body, None, None, &[]));
    let fonts = font_names(&pdf);
    assert!(fonts.iter().any(|f| f.starts_with("Didot")), "{fonts:?}");
    assert!(fonts.iter().any(|f| f.starts_with("TimesNewRoman")), "{fonts:?}");
    assert!(!fonts.iter().any(|f| f.contains("ArialUnicode")), "{fonts:?}");
    assert_eq!(text(&pdf), "Didot and Счёт");
}

#[test]
fn ghostscript_reads_the_same_text_from_the_textutil_document() {
    let Some(gs) = ghostscript() else {
        eprintln!("skipping: Ghostscript is not installed");
        return;
    };
    let bytes = std::fs::read("../tests/fixtures/basic/textutil-memo.docx").expect("fixture");
    let scratch = Scratch::new("gs");
    let (pdf, _) = convert(&scratch, &bytes);
    let output = Command::new(gs).args(["-q", "-dNOPAUSE", "-dBATCH", "-sDEVICE=txtwrite", "-sOutputFile=-"]).arg(&pdf).output().expect("gs");
    assert!(output.status.success());
    assert_eq!(collapse(&String::from_utf8_lossy(&output.stdout)), docx_text(&bytes));
}

#[tokio::test]
async fn a_vibepdf_word_export_comes_back_with_its_headings_and_image() {
    let scratch = Scratch::new("roundtrip");
    let exported = scratch.path("report.docx");
    let handle = DocumentActorHandle::spawn(None, uuid::Uuid::new_v4(), PathBuf::from("../tests/fixtures/basic/report.pdf"), None).expect("opens");
    handle.export_docx(vec![], exported.clone()).await.expect("export");
    let bytes = std::fs::read(&exported).expect("read");

    let (pdf, summary) = convert(&scratch, &bytes);
    assert_eq!(text(&pdf), docx_text(&bytes), "warnings: {:?}", summary.warnings);
    assert!(run_with(&pdf, "Quarterly Report").font_size > run_with(&pdf, "The quarter closed").font_size, "the heading lost its size");
    let doc = Document::load_mem(&std::fs::read(&pdf).expect("read")).expect("parse");
    assert!(
        doc.objects.values().filter_map(|o| o.as_stream().ok()).any(|s| s.dict.get(b"Subtype").and_then(Object::as_name).is_ok_and(|n| n == b"Image")),
        "the image didn't come back"
    );
}

/// `Sample PDFs/out/p7-create/` — the textutil memo and the round-tripped
/// report, for Preview. `cargo test --test create_docx -- --ignored`
#[test]
#[ignore = "writes verification files for a human to open"]
fn writes_the_verification_files() {
    let dir = PathBuf::from("../Sample PDFs/out/p7-create");
    std::fs::create_dir_all(&dir).expect("dir");
    let summary = create_pdf_from_files(&[PathBuf::from("../tests/fixtures/basic/textutil-memo.docx")], &dir.join("from-textutil.pdf"), PageSize::A4).expect("create");
    println!("VERIFY from-textutil.pdf: {summary:?}");
}
