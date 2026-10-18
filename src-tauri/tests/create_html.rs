//! SPEC: P7-OCR-009 (P7.C1c) — create a PDF from an HTML page.
//!
//! Three kinds of input. Hand-written pages, one feature each, so a failure
//! names the feature. Pages macOS's `textutil` wrote (Apple's "Cocoa HTML
//! Writer": class-based CSS, shorthands, real tables) — a writer we didn't
//! make. And round trips through VibePDF's own HTML export.
//!
//! What the tests trust: PDFium (and Ghostscript, where it's installed)
//! extract the text, and it must equal a crude tag-stripping reading of the
//! page that shares nothing with the reader under test; positions come from
//! each run's text matrix; colours, rules and borders from rendered pixels.

use std::path::{Path, PathBuf};
use std::process::Command;

use lopdf::{Document, Object};
use vibepdf_lib::error::CommandError;
use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::create::{create_pdf_from_files, CreateSummary, PageSize};
use vibepdf_lib::pdf::document::open_pdf;
use vibepdf_lib::pdf::render::{render_page, ImageFormat};
use vibepdf_lib::pdf::text_extract::{extract_text_runs, TextRun};

/// The page's left margin and text width on A4: 0.75 in margins.
const LEFT: f32 = 54.0;
const TEXT_WIDTH: f32 = 595.28 - 108.0;

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("vibepdf-html-in-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Self(dir)
    }
    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("dir");
        }
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

fn convert_file(scratch: &Scratch, input: &Path) -> (PathBuf, CreateSummary) {
    let dest = scratch.path("out.pdf");
    let summary = create_pdf_from_files(&[input.to_path_buf()], &dest, PageSize::A4).expect("create succeeds");
    (dest, summary)
}

/// `body` as a page's body, written as `page.html` and converted.
fn convert(scratch: &Scratch, body: &str) -> (PathBuf, CreateSummary) {
    let input = scratch.write("page.html", format!("<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>t</title></head><body>{body}</body></html>").as_bytes());
    convert_file(scratch, &input)
}

fn runs(path: &Path) -> Vec<Vec<TextRun>> {
    let (doc, meta) = open_pdf(path, None).expect("opens");
    (0..usize::try_from(meta.page_count).expect("count")).map(|p| extract_text_runs(&doc, p).expect("extract")).collect()
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The pages' text as a reader would take it: runs that touch on one
/// baseline joined directly, anything else separated by a space.
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

/// The run whose text contains `needle`'s first word.
fn run_with(path: &Path, needle: &str) -> TextRun {
    let first = needle.split_whitespace().next().unwrap_or(needle);
    runs(path).into_iter().flatten().find(|r| r.text.contains(first)).unwrap_or_else(|| panic!("no run with {first:?}"))
}

/// Where a run's text starts, and its baseline.
fn origin(run: &TextRun) -> (f32, f32) {
    (run.transform[4], run.transform[5])
}

fn near(actual: f32, expected: f32, what: &str) {
    assert!((actual - expected).abs() < 0.6, "{what}: {actual}, expected {expected}");
}

/// The first run on a page: on the highest baseline, leftmost.
fn first_run(page: &[TextRun]) -> &TextRun {
    page.iter()
        .min_by(|a, b| {
            let (a, b) = (origin(a), origin(b));
            b.1.round().total_cmp(&a.1.round()).then(a.0.total_cmp(&b.0))
        })
        .expect("text on the page")
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

fn image_count(path: &Path) -> usize {
    let doc = Document::load_mem(&std::fs::read(path).expect("read")).expect("parse");
    doc.objects
        .values()
        .filter_map(|o| o.as_stream().ok())
        .filter(|s| s.dict.get(b"Subtype").and_then(Object::as_name).is_ok_and(|n| n == b"Image"))
        .count()
}

/// Page `page` at 72 dpi: pixel (x, y) is point (x, y) from the top-left.
fn pixels(pdf: &Path, page: u32) -> impl Fn(u32, u32) -> [u8; 3] {
    let (doc, _) = open_pdf(pdf, None).expect("opens");
    let rendered = render_page(&doc, page, 72.0, ImageFormat::Rgba8).expect("render");
    move |x, y| {
        let i = usize::try_from((y * rendered.width + x) * 4).expect("index");
        [rendered.bytes[i], rendered.bytes[i + 1], rendered.bytes[i + 2]]
    }
}

fn is_white(p: [u8; 3]) -> bool {
    p.iter().all(|c| *c > 245)
}

fn close_to(p: [u8; 3], want: [u8; 3]) -> bool {
    p.iter().zip(want).all(|(a, b)| a.abs_diff(b) <= 40)
}

fn ghostscript() -> Option<&'static str> {
    ["/opt/homebrew/bin/gs", "/usr/local/bin/gs", "gs"]
        .into_iter()
        .find(|p| Command::new(p).arg("--version").output().is_ok_and(|o| o.status.success()))
}

/// The text a browser shows, read crudely and independently of the reader
/// under test: no `<head>`, `<script>` or `<style>`; tags removed (block tags
/// as spaces); the common entities decoded; whitespace collapsed.
fn html_text(html: &str) -> String {
    let mut html = html.to_owned();
    for name in ["head", "script", "style"] {
        loop {
            let lower = html.to_ascii_lowercase();
            let Some(start) = lower.find(&format!("<{name}")) else { break };
            let close = format!("</{name}>");
            let end = lower[start..].find(&close).map_or(html.len(), |e| start + e + close.len());
            html.replace_range(start..end, " ");
        }
    }
    const BLOCKS: [&str; 22] = [
        "p", "div", "br", "li", "tr", "td", "th", "h1", "h2", "h3", "h4", "h5", "h6", "table", "ul", "ol", "pre", "hr", "img", "caption", "dt", "dd",
    ];
    let mut out = String::new();
    let mut rest = html.as_str();
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let close = rest[open..].find('>').map_or(rest.len(), |c| open + c + 1);
        let name: String = rest[open + 1..close].trim_start_matches('/').chars().take_while(char::is_ascii_alphanumeric).collect::<String>().to_ascii_lowercase();
        if BLOCKS.contains(&name.as_str()) {
            out.push(' ');
        }
        rest = &rest[close..];
    }
    out.push_str(rest);
    let decoded = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&eacute;", "é")
        .replace("&amp;", "&");
    collapse(&decoded)
}

/// A PNG of `w` x `h` pixels in one colour.
fn png(w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
    let pixels: Vec<u8> = (0..w * h).flat_map(|_| rgb).collect();
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, w, h);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().expect("header").write_image_data(&pixels).expect("data");
    }
    bytes
}

// --- text and formatting ------------------------------------------------------

#[test]
fn headings_and_paragraphs_come_out_in_order_at_browser_sizes() {
    let scratch = Scratch::new("order");
    let (pdf, summary) = convert(&scratch, "<h1>Title here</h1><p>Body one.</p><h2>Sub heading</h2><p>Body two.</p>");
    assert_eq!(text(&pdf), "Title here Body one. Sub heading Body two.");
    assert!(summary.warnings.is_empty(), "{:?}", summary.warnings);
    // 16 px text, 2em and 1.5em headings, as every browser's defaults.
    near(run_with(&pdf, "Body one").font_size, 12.0, "body");
    near(run_with(&pdf, "Title").font_size, 24.0, "h1");
    near(run_with(&pdf, "Sub heading").font_size, 18.0, "h2");
    assert!(run_with(&pdf, "Title").font_name.contains("Bold"), "{}", run_with(&pdf, "Title").font_name);
    near(origin(&run_with(&pdf, "Body one")).0, LEFT, "the text starts at the margin");
}

#[test]
fn inline_formatting_uses_the_real_faces() {
    let scratch = Scratch::new("faces");
    let (pdf, _) = convert(
        &scratch,
        r#"<p style="font-family: Arial">plain <b>bold</b> <i>italic</i> <strong><em>both</em></strong></p>
           <p style="font-family: Arial"><u>Under</u></p><p style="font-family: Arial"><s>Struck</s></p>"#,
    );
    assert_eq!(font_names(&pdf), vec!["Arial-BoldItalicMT", "Arial-BoldMT", "Arial-ItalicMT", "ArialMT"]);
    assert_eq!(text(&pdf), "plain bold italic both Under Struck");
    let at = pixels(&pdf, 0);
    // A line under "Under" a little below the baseline, and through "Struck".
    let line_in = |word: &str, below: f32| {
        let run = run_with(&pdf, word);
        let (x0, x1) = (run.bbox[0] as u32 + 1, run.bbox[2] as u32 - 1);
        let y = (842.0 - origin(&run).1 + below).round() as u32;
        (y - 1..=y + 1).any(|y| (x0..x1).all(|x| at(x, y).iter().all(|c| *c < 200)))
    };
    assert!(line_in("Under", 1.5), "no underline");
    assert!(line_in("Struck", -3.4), "no strike-through");
}

#[test]
fn the_cascade_orders_rules_like_a_browser() {
    let scratch = Scratch::new("cascade");
    let body = r#"
        <style>
            p { color: #ff0000 }
            #i { color: #0000ff }
            .c { color: #00ff00 }
            a { color: #00aa00 }
            p.late { color: #00ff00 }
            p.late { color: #ff00ff }
            .imp { color: #00ffff !important }
            div.box p { color: #808080 }
        </style>
        <p>TypeRule</p>
        <p class="c">ClassRule</p>
        <p class="c" id="i">IdRule</p>
        <p class="late">LaterRule</p>
        <p class="c" style="color: #808000">InlineRule</p>
        <p class="imp" style="color: #000000">ImportantRule</p>
        <div style="color: #800080"><span>Inherited</span></div>
        <div class="box"><p>Descendant</p></div>
        <p><a href="x.html">LinkRule</a></p>"#;
    let (pdf, _) = convert(&scratch, body);
    for (word, colour) in [
        ("TypeRule", "#ff0000"),
        ("ClassRule", "#00ff00"),
        ("IdRule", "#0000ff"),
        ("LaterRule", "#ff00ff"),
        ("InlineRule", "#808000"),
        ("ImportantRule", "#00ffff"),
        ("Inherited", "#800080"),
        ("Descendant", "#808080"),
        // The page's `a` beats the browser's more specific `a[href]`: origin first.
        ("LinkRule", "#00aa00"),
    ] {
        assert_eq!(run_with(&pdf, word).color, colour, "{word}");
    }
}

#[test]
fn fonts_follow_the_family_list_and_the_shorthand() {
    let scratch = Scratch::new("families");
    let body = r#"
        <p style="font-family: 'No Such Font', Arial, sans-serif">Listed</p>
        <p style="font-family: monospace">Mono</p>
        <p style="font: italic bold 18px/2 Georgia, serif">Shorthand</p>
        <p style="font-family: sans-serif">Generic</p>"#;
    let (pdf, summary) = convert(&scratch, body);
    assert_eq!(run_with(&pdf, "Listed").font_name, "ArialMT");
    assert!(run_with(&pdf, "Mono").font_name.contains("Courier"), "{}", run_with(&pdf, "Mono").font_name);
    let shorthand = run_with(&pdf, "Shorthand");
    assert_eq!(shorthand.font_name, "Georgia-BoldItalic");
    near(shorthand.font_size, 13.5, "18px");
    assert_eq!(run_with(&pdf, "Generic").font_name, "ArialMT");
    assert!(!summary.warnings.iter().any(|w| w.contains("isn't installed")), "a listed fallback was installed: {:?}", summary.warnings);
}

#[test]
fn lengths_convert_from_px_em_rem_and_percent() {
    let scratch = Scratch::new("units");
    let body = r#"
        <p style="font-size: 20px">Pixels</p>
        <div style="font-size: 10pt"><span style="font-size: 2em">Ems</span></div>
        <p style="font-size: 150%">Percent</p>
        <div style="font-size: 30px"><span style="font-size: 2rem">Rems</span></div>
        <p style="font-size: 0.5in">Inches</p>"#;
    let (pdf, _) = convert(&scratch, body);
    near(run_with(&pdf, "Pixels").font_size, 15.0, "20px");
    near(run_with(&pdf, "Ems").font_size, 20.0, "2em of 10pt");
    near(run_with(&pdf, "Percent").font_size, 18.0, "150% of 12pt");
    near(run_with(&pdf, "Rems").font_size, 24.0, "2rem of the root's 12pt");
    near(run_with(&pdf, "Inches").font_size, 36.0, "0.5in");
}

#[test]
fn margins_collapse_and_indents_add_up() {
    let scratch = Scratch::new("margins");
    let body = r#"
        <p style="margin: 0">Cone</p><p style="margin: 0">Dtwo</p>
        <p style="margin: 0 0 20px">Abottom</p><p style="margin: 30px 0 0">Btop</p>
        <div style="margin-left: 1in"><p style="padding-left: 36pt">Indented</p></div>
        <blockquote>Quoted</blockquote>"#;
    let (pdf, _) = convert(&scratch, body);
    let y = |t: &str| origin(&run_with(&pdf, t)).1;
    let line = y("Cone") - y("Dtwo");
    assert!(line > 10.0, "{line}");
    // 20px under and 30px over collapse to 30px (22.5pt), not 50px.
    near(y("Abottom") - y("Btop") - line, 22.5, "the collapsed margin");
    near(origin(&run_with(&pdf, "Indented")).0, LEFT + 72.0 + 36.0, "margin plus padding");
    near(origin(&run_with(&pdf, "Quoted")).0, LEFT + 30.0, "a blockquote's 40px");
}

#[test]
fn text_align_and_indent() {
    let scratch = Scratch::new("align");
    let body = r#"
        <p style="text-align: center">Centred</p>
        <p align="right">Righted</p>
        <center>Old centre</center>
        <p style="text-indent: 2em">Indented first</p>"#;
    let (pdf, _) = convert(&scratch, body);
    let middle = |t: &str| {
        let r = run_with(&pdf, t);
        (r.bbox[0] + r.bbox[2]) / 2.0
    };
    assert!((middle("Centred") - (LEFT + TEXT_WIDTH / 2.0)).abs() < 2.0, "{}", middle("Centred"));
    assert!((middle("Old") - (LEFT + TEXT_WIDTH / 2.0)).abs() < 25.0, "{}", middle("Old"));
    assert!((run_with(&pdf, "Righted").bbox[2] - (LEFT + TEXT_WIDTH)).abs() < 2.0, "{:?}", run_with(&pdf, "Righted").bbox);
    near(origin(&run_with(&pdf, "Indented")).0, LEFT + 24.0, "2em of 12pt");
}

#[test]
fn case_shifts_and_line_height() {
    let scratch = Scratch::new("case");
    let body = r#"
        <p style="text-transform: uppercase">shouted words</p>
        <p style="text-transform: capitalize">title case here</p>
        <p>Hbase<sub>low</sub> and Xbase<sup>high</sup></p>
        <div style="line-height: 2"><p style="margin: 0">Lone<br>Ltwo</p></div>"#;
    let (pdf, _) = convert(&scratch, body);
    assert!(text(&pdf).starts_with("SHOUTED WORDS Title Case Here"), "{}", text(&pdf));
    let y = |t: &str| origin(&run_with(&pdf, t)).1;
    assert!(y("low") < y("Hbase") - 1.5, "subscript not lowered: {} vs {}", y("low"), y("Hbase"));
    assert!(y("high") > y("Xbase") + 3.0, "superscript not raised: {} vs {}", y("high"), y("Xbase"));
    assert!(run_with(&pdf, "low").font_size < 11.0, "<sub> is smaller");
    near(y("Lone") - y("Ltwo"), 24.0, "line-height: 2 of 12pt");
}

#[test]
fn lists_get_markers_and_nest() {
    let scratch = Scratch::new("lists");
    let body = r#"
        <ul><li>Uone</li><li>Utwo<ul><li>Inner</li></ul></li></ul>
        <ol start="3" type="i"><li>Third</li><li>Fourth</li></ol>
        <ol style="list-style-type: upper-alpha"><li>Alpha</li></ol>"#;
    let (pdf, _) = convert(&scratch, body);
    assert_eq!(text(&pdf), "\u{2022} Uone \u{2022} Utwo \u{25E6} Inner iii. Third iv. Fourth A. Alpha");
    near(origin(&run_with(&pdf, "Uone")).0, LEFT + 30.0, "a list's 40px");
    near(origin(&run_with(&pdf, "Inner")).0, LEFT + 60.0, "a nested list's");
    let bullet = runs(&pdf).concat().into_iter().find(|r| r.text.contains('\u{2022}')).expect("a bullet");
    assert!(bullet.bbox[2] < LEFT + 30.0, "the marker hangs left of the text: {:?}", bullet.bbox);
}

#[test]
fn pre_keeps_spaces_and_line_breaks() {
    let scratch = Scratch::new("pre");
    let (pdf, _) = convert(&scratch, "<pre>a        b\n    y\n</pre><p>After</p>");
    let all = runs(&pdf).concat();
    // PDFium's extraction collapses runs of spaces, so they're measured: eight
    // monospaced spaces between a and b, not one.
    let first = all.iter().find(|r| r.text.starts_with('a')).expect("first line");
    let em = first.font_size * 0.6;
    assert!(first.bbox[2] - first.bbox[0] > 9.0 * em, "spaces collapsed: {:?}", first.bbox);
    let second = all.iter().find(|r| r.text.contains('y')).expect("second line");
    assert!(origin(second).1 < origin(first).1 - 5.0, "no line break");
    assert!(first.font_name.contains("Courier"), "{}", first.font_name);
    // The trailing newline makes no empty line: After follows directly.
    let after = run_with(&pdf, "After");
    assert!(origin(second).1 - origin(&after).1 < 40.0);
}

#[test]
fn whitespace_collapses_and_entities_decode() {
    let scratch = Scratch::new("whitespace");
    let (pdf, _) = convert(&scratch, "<p>  Two   spaces\n and&nbsp;nbsp &amp; &lt;tag&gt; caf&eacute;</p><p>line<br>next</p>");
    assert_eq!(text(&pdf), "Two spaces and nbsp & <tag> café line next");
    assert!(origin(&run_with(&pdf, "next")).1 < origin(&run_with(&pdf, "line")).1 - 5.0, "<br> broke no line");

    // PDFium's extraction collapses spaces too, so widths show what was drawn;
    // and a <br> that ends a paragraph makes no empty line.
    let (pdf, _) = convert(
        &scratch,
        r#"<p style="margin: 0">Aa   <b></b>   bb</p><p style="margin: 0">Cc bb</p>
           <p style="margin: 0">Eone<br></p><p style="margin: 0">Etwo</p><p style="margin: 0">Ethree</p>"#,
    );
    let width = |t: &str| {
        let r = run_with(&pdf, t);
        r.bbox[2] - r.bbox[0]
    };
    assert!((width("Aa") - width("Cc")).abs() < 1.0, "spaces weren't collapsed: {} vs {}", width("Aa"), width("Cc"));
    let y = |t: &str| origin(&run_with(&pdf, t)).1;
    near(y("Eone") - y("Etwo"), y("Etwo") - y("Ethree"), "the trailing <br>");
}

#[test]
fn a_horizontal_rule_is_drawn_between() {
    let scratch = Scratch::new("hr");
    let (pdf, _) = convert(&scratch, "<p>Above</p><hr><p>Below</p>");
    let (top, bottom) = (842.0 - origin(&run_with(&pdf, "Above")).1, 842.0 - origin(&run_with(&pdf, "Below")).1);
    let at = pixels(&pdf, 0);
    let rule = (top as u32 + 4..bottom as u32 - 12).find(|&y| (100..500).all(|x| !is_white(at(x, y))));
    assert!(rule.is_some(), "no rule across the page between the paragraphs");
}

#[test]
fn hidden_content_scripts_and_styles_are_not_shown() {
    let scratch = Scratch::new("hidden");
    let body = r#"<p>Shown</p><p style="display: none">Hidden</p><script>var s = "Script";</script>
        <style>p { }</style><p hidden>Attribute</p><template><p>Template</p></template>
        <p style="visibility: hidden">Invisible</p><noscript>NoScript</noscript>"#;
    let (pdf, _) = convert(&scratch, body);
    // Scripts never run, so <noscript> shows, as with scripting off.
    assert_eq!(text(&pdf), "Shown NoScript");
}

#[test]
fn a_page_break_starts_a_new_page() {
    let scratch = Scratch::new("break");
    let (pdf, _) = convert(&scratch, r#"<p>One</p><p style="page-break-before: always">Two</p><p style="break-after: page">Three</p><p>Four</p>"#);
    let pages = runs(&pdf);
    assert_eq!(pages.len(), 3);
    assert_eq!(first_run(&pages[1]).text.trim(), "Two");
    assert_eq!(first_run(&pages[2]).text.trim(), "Four");
}

// --- tables -------------------------------------------------------------------

#[test]
fn a_table_sizes_its_columns_from_its_content() {
    let scratch = Scratch::new("auto");
    let body = r#"
        <table><tr><td>x</td><td>Ytiny</td></tr></table>
        <table><tr><td>Nshort</td><td>Wlong words that go on and on, more than the line can hold at all, and then some more words to be sure it wraps</td></tr></table>
        <table style="width: 100%"><tr><td style="width: 25%">Quarter</td><td>Rest</td></tr></table>"#;
    let (pdf, _) = convert(&scratch, body);
    let x = |t: &str| origin(&run_with(&pdf, t)).0;
    // A table with little in it is only as wide as that.
    assert!(x("Ytiny") < LEFT + 20.0, "{}", x("Ytiny"));
    // The short column stays short; the long one takes the rest and wraps.
    assert!(x("Wlong") < LEFT + 60.0, "{}", x("Wlong"));
    assert!(runs(&pdf).concat().iter().all(|r| r.bbox[2] <= LEFT + TEXT_WIDTH + 1.0), "a line ran past the margin");
    near(x("Rest"), LEFT + TEXT_WIDTH * 0.25 + 0.75, "25% of the width, then 1px padding");
}

#[test]
fn borders_padding_and_backgrounds_come_from_css_and_attributes() {
    let scratch = Scratch::new("cells");
    let body = r##"
        <table style="border-collapse: collapse"><tr>
            <td style="border: 2px solid #ff0000; padding: 10px; background: #00c000; width: 100px; height: 40px">Green</td>
            <td style="padding: 10px">Plain</td>
        </tr></table>
        <table border="1" cellpadding="8"><tr><td>Bordered</td></tr></table>
        <table bgcolor="#0000c0"><tr bgcolor="#c00000"><td>RowFill</td></tr><tr><td>TableFill</td></tr></table>"##;
    let (pdf, _) = convert(&scratch, body);
    let green = run_with(&pdf, "Green");
    near(origin(&green).0, LEFT + 1.5 + 7.5, "a 2px border, then 10px padding");
    let at = pixels(&pdf, 0);
    let y = (842.0 - origin(&green).1 - 4.0) as u32;
    assert!(close_to(at(LEFT as u32, y), [255, 0, 0]), "no red left border: {:?}", at(LEFT as u32, y));
    assert!(close_to(at(LEFT as u32 + 60, y), [0, 192, 0]), "no fill: {:?}", at(LEFT as u32 + 60, y));
    // 100px of content plus 10px of padding each side: the next cell starts at 90pt.
    near(origin(&run_with(&pdf, "Plain")).0, LEFT + 1.5 + 75.0 + 15.0 + 1.5 + 7.5, "the next column");
    near(origin(&run_with(&pdf, "Bordered")).0, LEFT + 0.75 + 6.0, "border=1, then cellpadding=8");
    let bordered = run_with(&pdf, "Bordered");
    let by = (842.0 - origin(&bordered).1) as u32;
    assert!((LEFT as u32 - 1..=LEFT as u32 + 1).any(|x| !is_white(at(x, by))), "border=1 drew no border");
    // The table's own border goes round its outer cells.
    let (pdf, _) = convert(&scratch, r##"<table style="border: 2px solid #0000ff"><tr><td style="padding: 20px">Edged</td><td style="padding: 20px">Edgedb</td></tr></table>"##);
    let at = pixels(&pdf, 0);
    let (a, b) = (run_with(&pdf, "Edged "), run_with(&pdf, "Edgedb"));
    let (left, right) = (LEFT as u32, (b.bbox[2] + 15.0 + 0.75) as u32);
    let mid = (842.0 - origin(&a).1 - 3.0) as u32;
    let (top, bottom) = (mid - 22, mid + 22);
    let edge = |x0: u32, x1: u32, y0: u32, y1: u32| (x0..=x1).any(|x| (y0..=y1).any(|y| close_to(at(x, y), [0, 0, 255])));
    assert!(edge(left - 1, left + 1, mid, mid), "left");
    assert!(edge(right - 2, right + 2, mid, mid), "right");
    assert!(edge(LEFT as u32 + 20, LEFT as u32 + 20, top - 4, top + 4), "top");
    assert!(edge(LEFT as u32 + 20, LEFT as u32 + 20, bottom - 4, bottom + 4), "bottom");
    let (pdf, _) = convert(&scratch, body);
    let at = pixels(&pdf, 0);

    // A cell without a background takes its row's, then its table's.
    for (word, colour) in [("RowFill", [192, 0, 0]), ("TableFill", [0, 0, 192])] {
        let run = run_with(&pdf, word);
        let (fx, fy) = ((run.bbox[2] + 1.0) as u32, (842.0 - origin(&run).1 - 3.0) as u32);
        assert!(close_to(at(fx, fy), colour), "{word}: {:?}", at(fx, fy));
    }
}

#[test]
fn colspan_and_rowspan_cover_their_cells() {
    let scratch = Scratch::new("spans");
    let body = r#"
        <table border="1" style="width: 300px">
            <tr><td colspan="2" style="text-align: center">Across</td></tr>
            <tr><td rowspan="3">Merged</td><td>Rone</td></tr>
            <tr><td>Rtwo</td></tr>
            <tr><td>Rthree</td></tr>
            <tr><td>Last</td><td>Lastb</td></tr>
        </table>"#;
    let (pdf, _) = convert(&scratch, body);
    assert_eq!(text(&pdf), "Across Merged Rone Rtwo Rthree Last Lastb");
    let y = |t: &str| origin(&run_with(&pdf, t)).1;
    // Middle-aligned (a cell's default) over three equal rows: level with the middle one.
    near(y("Merged"), y("Rtwo"), "Merged");
    // The cells beside the merge sit in the second column, as Lastb does.
    near(origin(&run_with(&pdf, "Rtwo")).0, origin(&run_with(&pdf, "Lastb")).0, "the second column");
    let across = run_with(&pdf, "Across");
    near((across.bbox[0] + across.bbox[2]) / 2.0, LEFT + 112.5, "centred across both columns");
}

#[test]
fn a_table_head_repeats_on_every_page() {
    let scratch = Scratch::new("thead");
    let rows: String = (1..=90).map(|i| format!("<tr><td>Row{i}</td><td>v{i}</td></tr>")).collect();
    let (pdf, _) = convert(&scratch, &format!("<table><thead><tr><th>Head</th><th>Value</th></tr></thead><tbody>{rows}</tbody></table>"));
    let pages = runs(&pdf);
    assert!(pages.len() >= 2, "{} pages", pages.len());
    for (n, page) in pages.iter().enumerate() {
        assert_eq!(first_run(page).text.trim(), "Head", "page {}", n + 1);
    }
    let order: Vec<String> = pages.concat().into_iter().map(|r| r.text.trim().to_owned()).filter(|t| t.starts_with("Row")).collect();
    assert_eq!(order, (1..=90).map(|i| format!("Row{i}")).collect::<Vec<_>>());
}

// --- images and other files ---------------------------------------------------------

#[test]
fn images_load_from_the_folder_and_from_data_uris() {
    use base64::Engine as _;
    let scratch = Scratch::new("images");
    scratch.write("page_files/red.png", &png(40, 20, [220, 30, 30]));
    let blue = base64::engine::general_purpose::STANDARD.encode(png(10, 10, [30, 30, 220]));
    let body = format!(
        r#"<p>Before</p><img src="page_files/red.png" width="200"><img src="data:image/png;base64,{blue}" style="width: 1in"><p>After</p>"#
    );
    let (pdf, summary) = convert(&scratch, &body);
    assert!(summary.warnings.is_empty(), "{:?}", summary.warnings);
    assert_eq!(image_count(&pdf), 2);
    let at = pixels(&pdf, 0);
    let below = (842.0 - origin(&run_with(&pdf, "Before")).1) as u32 + 8;
    // 200px wide is 150pt; 40x20 keeps its shape, so 75pt tall.
    let red_rows: Vec<u32> = (below..below + 120).filter(|&y| close_to(at(LEFT as u32 + 75, y), [220, 30, 30])).collect();
    assert!((red_rows.len() as f32 - 75.0).abs() <= 2.0, "red is {} rows tall", red_rows.len());
    assert!(close_to(at(LEFT as u32 + 148, red_rows[10]), [220, 30, 30]) && is_white(at(LEFT as u32 + 153, red_rows[10])), "not 150pt wide");
    let blue_top = red_rows.last().copied().unwrap_or(0) + 1;
    let blue_rows = (blue_top..blue_top + 100).filter(|&y| close_to(at(LEFT as u32 + 36, y), [30, 30, 220])).count();
    assert!((blue_rows as f32 - 72.0).abs() <= 2.0, "1in is {blue_rows} rows");
}

#[test]
fn remote_and_outside_files_are_never_read() {
    let scratch = Scratch::new("offline");
    scratch.write("outside.png", &png(4, 4, [0, 0, 0]));
    let input = scratch.write(
        "inner/page.html",
        br#"<html><head><link rel="stylesheet" href="https://example.com/s.css"></head><body>
            <img src="https://example.com/x.png" alt="RemoteAlt">
            <img src="//example.com/y.png" alt="SchemeAlt">
            <img src="../outside.png" alt="OutsideAlt">
            <img src="missing.png" alt="MissingAlt">
            <p>Text</p></body></html>"#,
    );
    let (pdf, summary) = convert_file(&scratch, &input);
    assert_eq!(image_count(&pdf), 0);
    assert_eq!(text(&pdf), "RemoteAlt SchemeAlt OutsideAlt MissingAlt Text");
    let warned = |needle: &str| summary.warnings.iter().any(|w| w.contains(needle));
    assert!(warned("doesn't go online"), "{:?}", summary.warnings);
    assert!(warned("outside the page's own folder"), "{:?}", summary.warnings);
    assert!(warned("missing.png wasn't found"), "{:?}", summary.warnings);
    // A web address is never looked for on disk.
    assert!(!warned("x.png") && !warned("y.png"), "{:?}", summary.warnings);
    let (_, summary) = convert(&scratch, r#"<img src="https://example.com/only.png" alt="Only">"#);
    assert!(summary.warnings.iter().any(|w| w.contains("doesn't go online")), "{:?}", summary.warnings);
}

#[test]
fn a_local_stylesheet_is_applied() {
    let scratch = Scratch::new("sheet");
    scratch.write("css/site.css", b"p.sheet { color: #ff0000 } @media screen { p.sheet { color: #00ff00 } }");
    let input = scratch.write("page.html", br#"<link rel="stylesheet" href="css/site.css"><p class="sheet">Styled</p>"#);
    let (pdf, _) = convert_file(&scratch, &input);
    assert_eq!(run_with(&pdf, "Styled").color, "#ff0000");
}

#[test]
fn what_is_simplified_is_reported() {
    let scratch = Scratch::new("simplified");
    let body = r#"<div style="float: left">Floated</div><div style="display: flex">Flexed</div>
        <svg><text>Drawn</text></svg><iframe src="x.html"></iframe><input value="Field"><p>Kept</p>
        <img src="data:image/gif;base64,R0lGODlhAQABAAAAACw=" alt="Gif">"#;
    let (pdf, summary) = convert(&scratch, body);
    assert_eq!(text(&pdf), "Floated Flexed Kept Gif");
    for needle in ["floats, positioning", "SVG, canvas, iframes", "Form fields", "such as GIF"] {
        assert!(summary.warnings.iter().any(|w| w.contains(needle)), "{needle}: {:?}", summary.warnings);
    }
    assert_eq!(summary.warnings.len(), 4, "each once: {:?}", summary.warnings);
}

#[test]
fn each_kind_of_page_layout_is_reported() {
    let scratch = Scratch::new("layout");
    for style in ["float: right", "position: absolute", "display: grid", "display: inline-flex", "column-count: 2"] {
        let (pdf, summary) = convert(&scratch, &format!(r#"<div style="{style}">Laid</div><p>Out</p>"#));
        assert_eq!(text(&pdf), "Laid Out", "{style}");
        assert!(summary.warnings.iter().any(|w| w.contains("source order")), "{style}: {:?}", summary.warnings);
    }
    let (_, summary) = convert(&scratch, r#"<div style="position: relative; float: none; display: block; columns: 1">Plain</div>"#);
    assert!(summary.warnings.is_empty(), "{:?}", summary.warnings);
}

#[test]
fn a_declared_or_marked_encoding_is_read() {
    let scratch = Scratch::new("encoding");
    let input = scratch.write("latin.html", b"<meta charset=\"windows-1252\"><p>caf\xE9 \x80</p>");
    let (pdf, summary) = convert_file(&scratch, &input);
    assert_eq!(text(&pdf), "café €");
    assert!(summary.warnings.is_empty(), "{:?}", summary.warnings);
    let mut utf16 = vec![0xFF, 0xFE];
    utf16.extend("<p>Ünïcödé</p>".encode_utf16().flat_map(u16::to_le_bytes));
    let input = scratch.write("wide.html", &utf16);
    let dest = scratch.path("wide.pdf");
    create_pdf_from_files(&[input], &dest, PageSize::A4).expect("create");
    assert_eq!(text(&dest), "Ünïcödé");
}

#[test]
fn a_binary_file_is_refused_by_name() {
    let scratch = Scratch::new("binary");
    let input = scratch.write("photo.html", b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR");
    let dest = scratch.path("out.pdf");
    match create_pdf_from_files(&[input], &dest, PageSize::A4) {
        Err(CommandError::InvalidInput(m)) => assert!(m.starts_with("photo.html"), "{m}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(!dest.exists());
}

#[test]
fn very_deep_nesting_is_flattened_not_a_crash() {
    let scratch = Scratch::new("deep");
    let depth = 100_000;
    let body = format!("{}Deep{}<p>After</p>", "<div>".repeat(depth), "</div>".repeat(depth));
    let (pdf, summary) = convert(&scratch, &body);
    assert_eq!(text(&pdf), "Deep After");
    assert!(summary.warnings.iter().any(|w| w.contains("deeply nested")), "{:?}", summary.warnings);
}

#[test]
fn deeply_nested_tables_and_lists_are_flattened_too() {
    // The pre-scan counts tables and lists, not the rows, cells and items the
    // parser adds between them, so the tree is deeper than it counted.
    let scratch = Scratch::new("deep-tables");
    let depth = 3_000;
    let body = format!(
        "{}Cell{}<ul>{}Item<p>After</p><script>var hidden = 1;</script>",
        "<table><tr><td>".repeat(depth),
        "</td></tr></table>".repeat(depth),
        "<li><ul>".repeat(depth)
    );
    let (pdf, summary) = convert(&scratch, &body);
    let text = text(&pdf);
    assert!(text.starts_with("Cell") && text.ends_with("Item After"), "{text}");
    assert!(summary.warnings.iter().any(|w| w.contains("deeply nested")), "{:?}", summary.warnings);
}

// --- pages we did not write -----------------------------------------------------

#[test]
fn a_page_from_macos_textutil_keeps_its_text_faces_and_layout() {
    let fixture = PathBuf::from("../tests/fixtures/basic/textutil-memo.cocoa.html");
    let html = std::fs::read_to_string(&fixture).expect("fixture");
    let scratch = Scratch::new("cocoa-memo");
    let (pdf, summary) = convert_file(&scratch, &fixture);
    assert_eq!(text(&pdf), html_text(&html), "warnings: {:?}", summary.warnings);
    assert!(summary.warnings.is_empty(), "{:?}", summary.warnings);
    // `font: 24.0px Times` over `font: 12.0px Times`.
    near(run_with(&pdf, "Quarterly memo").font_size, 18.0, "24px");
    near(run_with(&pdf, "Supply lead").font_size, 9.0, "12px");
    assert!(run_with(&pdf, "Quarterly memo").font_name.starts_with("Times-Bold"), "{}", run_with(&pdf, "Quarterly memo").font_name);
    assert_eq!(run_with(&pdf, "Red words").color, "#b00004");
    let centred = run_with(&pdf, "Centred line");
    assert!(((centred.bbox[0] + centred.bbox[2]) / 2.0 - (LEFT + TEXT_WIDTH / 2.0)).abs() < 3.0, "{:?}", centred.bbox);
    // p.p4's 36px left margin indents the list lines.
    assert!(run_with(&pdf, "Supply lead").bbox[0] > LEFT + 27.0);
    assert!(font_names(&pdf).iter().all(|f| f.starts_with("Times")), "{:?}", font_names(&pdf));
}

#[test]
fn a_table_from_macos_textutil_keeps_its_columns() {
    let fixture = PathBuf::from("../tests/fixtures/basic/textutil-table.cocoa.html");
    let html = std::fs::read_to_string(&fixture).expect("fixture");
    let scratch = Scratch::new("cocoa-table");
    let (pdf, summary) = convert_file(&scratch, &fixture);
    assert_eq!(text(&pdf), html_text(&html), "warnings: {:?}", summary.warnings);
    let all = runs(&pdf).concat();
    let x = |t: &str| origin(all.iter().find(|r| r.text.trim() == t).unwrap_or_else(|| panic!("no {t:?}"))).0;
    // Each column's cells start together: 1px border and 4px padding in.
    for cells in [["Bearing housing", "Drive shaft", "Fasteners, assorted"], ["6 weeks", "9 weeks", "2 weeks"]] {
        for cell in cells {
            near(x(cell), x(cells[0]), cell);
        }
    }
    near(x("Bearing housing"), LEFT + 0.75 + 3.0, "the first column");
    // The headers are centred over their columns, in order.
    assert!(x("Part") < x("Supplier") && x("Supplier") < x("Lead time") && x("Lead time") < x("6 weeks") + 10.0);
    // td.td4's background, behind "Drive shaft".
    let shaft = all.iter().find(|r| r.text.contains("Drive")).expect("shaft");
    let at = pixels(&pdf, 0);
    let (sx, sy) = ((shaft.bbox[0] - 2.0) as u32, (842.0 - origin(shaft).1 - 2.0) as u32);
    assert!(close_to(at(sx, sy), [255, 255, 11]), "no yellow behind Drive shaft: {:?}", at(sx, sy));
}

#[test]
fn ghostscript_reads_the_same_text_from_the_textutil_page() {
    let Some(gs) = ghostscript() else {
        eprintln!("skipping: Ghostscript is not installed");
        return;
    };
    let fixture = PathBuf::from("../tests/fixtures/basic/textutil-table.cocoa.html");
    let html = std::fs::read_to_string(&fixture).expect("fixture");
    let scratch = Scratch::new("gs");
    let (pdf, _) = convert_file(&scratch, &fixture);
    let output = Command::new(gs).args(["-q", "-dNOPAUSE", "-dBATCH", "-sDEVICE=txtwrite", "-sOutputFile=-"]).arg(&pdf).output().expect("gs");
    assert!(output.status.success());
    assert_eq!(collapse(&String::from_utf8_lossy(&output.stdout)), html_text(&html));
}

async fn html_export_round_trip(fixture: &str, tag: &str) -> (Scratch, PathBuf, String, CreateSummary) {
    let scratch = Scratch::new(tag);
    let exported = scratch.path("export/page.html");
    std::fs::create_dir_all(exported.parent().expect("parent")).expect("dir");
    let handle = DocumentActorHandle::spawn(None, uuid::Uuid::new_v4(), PathBuf::from(fixture), None).expect("opens");
    handle.export_html(vec![], exported.clone()).await.expect("export");
    let html = std::fs::read_to_string(&exported).expect("read");
    let dest = scratch.path("back.pdf");
    let summary = create_pdf_from_files(&[exported], &dest, PageSize::A4).expect("create");
    (scratch, dest, html, summary)
}

#[tokio::test]
async fn a_vibepdf_html_export_comes_back_with_its_headings_and_image() {
    let (_scratch, pdf, html, summary) = html_export_round_trip("../tests/fixtures/basic/report.pdf", "roundtrip-report").await;
    assert_eq!(text(&pdf), html_text(&html), "warnings: {:?}", summary.warnings);
    assert!(summary.warnings.is_empty(), "{:?}", summary.warnings);
    assert!(run_with(&pdf, "Quarterly Report").font_size > run_with(&pdf, "The quarter closed").font_size, "the heading lost its size");
    assert_eq!(image_count(&pdf), 1, "the image in report_files/ didn't come back");
}

#[tokio::test]
async fn a_vibepdf_html_export_of_a_table_comes_back_as_a_table() {
    let (_scratch, pdf, html, summary) = html_export_round_trip("../tests/fixtures/basic/table.pdf", "roundtrip-table").await;
    assert_eq!(text(&pdf), html_text(&html), "warnings: {:?}", summary.warnings);
    let page = &runs(&pdf)[0];
    let x = |t: &str| origin(page.iter().find(|r| r.text.trim() == t).unwrap_or_else(|| panic!("no {t:?} on page 1"))).0;
    let columns = [["Region", "North", "South", "Overseas"], ["Revenue", "412,000", "298,500", "77,250"], ["Change", "+6%", "-2%", "+31%"], ["Units", "1240", "980", "310"]];
    let mut previous = 0.0;
    for column in columns {
        let start = x(column[0]);
        for cell in column {
            near(x(cell), start, cell);
        }
        assert!(start > previous + 20.0, "{} isn't right of the column before", column[0]);
        previous = start;
    }
}

/// `Sample PDFs/out/p7-create/` — the textutil pages and a hand-written page
/// of features, for Preview. `cargo test --test create_html -- --ignored`
#[test]
#[ignore = "writes verification files for a human to open"]
fn writes_the_verification_files() {
    let dir = PathBuf::from("../Sample PDFs/out/p7-create");
    std::fs::create_dir_all(&dir).expect("dir");
    for name in ["textutil-memo", "textutil-table"] {
        let source = PathBuf::from(format!("../tests/fixtures/basic/{name}.cocoa.html"));
        let summary = create_pdf_from_files(&[source], &dir.join(format!("from-html-{name}.pdf")), PageSize::A4).expect("create");
        println!("VERIFY from-html-{name}.pdf: {summary:?}");
    }
    let rows: String = (1..=50).map(|i| format!("<tr><td>Item {i}</td><td>Note for item {i}</td></tr>")).collect();
    let page = format!(
        r#"<!DOCTYPE html><html><head><meta charset="utf-8"><style>
            body {{ font-family: Georgia, serif }}
            h1 {{ color: #1f4e79 }}
            .note {{ background: #fff3c4; padding: 6px; border: 1px solid #c9a227 }}
            table {{ border-collapse: collapse }}
            th, td {{ border: 1px solid #888; padding: 4px 8px }}
            th {{ background: #d9e2f3 }}
        </style></head><body>
        <h1>HTML to PDF (P7.C1c)</h1>
        <p>A paragraph with <b>bold</b>, <i>italic</i>, <u>underlined</u>, <s>struck</s>, <code>code</code>, H<sub>2</sub>O and x<sup>2</sup>.</p>
        <ul><li>A bullet</li><li>Another, with a nested list<ol><li>first</li><li>second</li></ol></li></ul>
        <pre>  preformatted
    text keeps its    spaces</pre>
        <hr>
        <table class="note"><tr><td>A table cell with a yellow background, and a table that spans
        two columns below:</td></tr></table>
        <table><thead><tr><th>Item</th><th>Note</th></tr></thead><tbody>
        <tr><td colspan="2" style="text-align: center">Across both columns</td></tr>{rows}</tbody></table>
        </body></html>"#
    );
    let source = dir.join("html-features.html");
    std::fs::write(&source, page).expect("write");
    let summary = create_pdf_from_files(&[source], &dir.join("from-html-features.pdf"), PageSize::A4).expect("create");
    println!("VERIFY from-html-features.pdf: {summary:?}");
}
