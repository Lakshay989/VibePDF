//! SPEC: P7-OCR-007 — "WHEN the user exports a PDF to HTML, THE system SHALL
//! produce semantic HTML with images extracted to a sibling folder."
//!
//! Every test reads the written file. The structure comes from the same blocks
//! the Word export uses (`document_blocks`), so these mirror its suites on the
//! same fixtures: headings told apart by level, bold and italic, escaping,
//! reading order, ruled tables. What is particular to HTML is checked here too:
//! the images exist where the page links them, a file name with a space still
//! links, and nothing is fetched from anywhere.
//!
//! `xmllint --html` is the outside opinion — an HTML parser we did not write —
//! and skips loudly where it is absent. A browser is the final word, in the
//! acceptance pass.

use std::path::{Path, PathBuf};

use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::export_html::HtmlExportSummary;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

/// A fresh directory; the export goes to `<dir>/<file_name>`.
fn scratch(tag: &str, file_name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vibepdf-html-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir.join(file_name)
}

async fn export_to(name: &str, dest: &Path) -> (String, HtmlExportSummary) {
    let handle = DocumentActorHandle::spawn(None, uuid::Uuid::new_v4(), fixture(name), None).expect("opens");
    let summary = handle.export_html(vec![], dest.to_path_buf()).await.expect("export succeeds");
    let html = std::fs::read_to_string(dest).expect("the file exists and is UTF-8");
    (html, summary)
}

async fn export(name: &str) -> (String, HtmlExportSummary, PathBuf) {
    let dest = scratch(name, "out.html");
    let (html, summary) = export_to(name, &dest).await;
    (html, summary, dest)
}

fn cleanup(dest: &Path) {
    if let Some(parent) = dest.parent() {
        let _ = std::fs::remove_dir_all(parent);
    }
}

/// Every `src="…"` in the page, in order.
fn image_sources(html: &str) -> Vec<String> {
    html.split(r#"<img src=""#)
        .skip(1)
        .map(|rest| rest.split('"').next().unwrap_or("").to_string())
        .collect()
}

/// A relative URL's path back to the bytes it names: `%XX` decoded.
fn percent_decode(url: &str) -> String {
    let bytes = url.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).expect("ascii");
            out.push(u8::from_str_radix(hex, 16).expect("hex"));
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).expect("UTF-8")
}

/// The text of the element that contains `needle`, from its opening tag.
fn element_around<'a>(html: &'a str, needle: &str) -> &'a str {
    let at = html.find(needle).unwrap_or_else(|| panic!("missing {needle}"));
    let start = html[..at].rfind('\n').map_or(0, |n| n + 1);
    &html[start..at + needle.len()]
}

#[tokio::test]
async fn the_page_is_a_complete_utf8_document() {
    let (html, _, dest) = export("report.pdf").await;
    assert!(html.starts_with("<!DOCTYPE html>\n"), "no doctype");
    assert!(html.contains(r#"<meta charset="utf-8">"#));
    assert!(html.contains("<title>") && html.contains("</title>"));
    assert!(html.trim_end().ends_with("</html>"));
    cleanup(&dest);
}

#[tokio::test]
async fn headings_are_headings_at_their_levels() {
    // The fixture sets 20/14/11 pt over 9 pt body: three levels, four headings.
    let (html, summary, dest) = export("report.pdf").await;
    assert_eq!(summary.headings, 4);
    assert!(element_around(&html, "Quarterly Report").starts_with("<h1>"), "the title is not the h1");
    assert_eq!(html.matches("<h1>").count(), 1);
    assert!(html.contains("<h2>") && html.contains("<h3>"), "levels were not told apart");
    cleanup(&dest);
}

#[tokio::test]
async fn bold_and_italic_become_strong_and_em() {
    let (html, _, dest) = export("report.pdf").await;
    assert!(element_around(&html, "This sentence is emphasised.").ends_with("<em>This sentence is emphasised."), "italic lost");
    assert!(element_around(&html, "This sentence is strong.").ends_with("<strong>This sentence is strong."), "bold lost");
    let plain = element_around(&html, "The quarter closed ahead of plan");
    assert!(!plain.contains("<strong>") && !plain.contains("<em>"), "plain text was marked up: {plain}");
    cleanup(&dest);
}

#[tokio::test]
async fn special_characters_are_escaped() {
    // `&`, `<` and `"` are in the fixture for this. Text from a PDF is
    // untrusted, and unescaped it becomes markup.
    let (html, _, dest) = export("report.pdf").await;
    assert!(html.contains("Revenue &amp; Costs"), "ampersand not escaped");
    assert!(html.contains("&lt;5%"), "less-than not escaped");
    assert!(html.contains("&quot;critical&quot;"), "quote not escaped");
    cleanup(&dest);
}

#[tokio::test]
async fn images_go_to_a_sibling_folder_and_resolve() {
    let (html, summary, dest) = export("report.pdf").await;
    assert_eq!(summary.images, 1);
    assert_eq!(summary.images_folder.as_deref(), Some("out_files"));

    let sources = image_sources(&html);
    assert_eq!(sources, vec!["out_files/image-001.png"]);
    let png = std::fs::read(dest.with_file_name(percent_decode(&sources[0]))).expect("the linked image exists");
    assert_eq!(&png[..4], b"\x89PNG", "the linked file is not a PNG");
    // Drawn at 128 x 96 pt, which is 171 x 128 CSS pixels.
    assert!(html.contains(r#"width="171" height="128""#), "not the drawn size: {html}");
    cleanup(&dest);
}

#[tokio::test]
async fn a_name_with_spaces_still_links_its_images() {
    let dest = scratch("spaces", "Q3 report #2.html");
    let (html, summary) = export_to("report.pdf", &dest).await;
    assert_eq!(summary.images_folder.as_deref(), Some("Q3 report #2_files"));
    let sources = image_sources(&html);
    assert_eq!(sources, vec!["Q3%20report%20%232_files/image-001.png"]);
    assert!(dest.with_file_name(percent_decode(&sources[0])).is_file(), "the link does not resolve");
    cleanup(&dest);
}

#[tokio::test]
async fn no_images_means_no_folder() {
    let (html, summary, dest) = export("hello.pdf").await;
    assert_eq!(summary.images, 0);
    assert_eq!(summary.images_folder, None);
    assert!(!dest.with_file_name("out_files").exists(), "an empty images folder was created");
    assert!(!html.contains("<img"));
    cleanup(&dest);
}

// An existing folder of the same name may hold someone's files. Only
// `image-NNN.png` is written into it; nothing else is touched.
#[tokio::test]
async fn an_existing_folder_keeps_its_other_files() {
    let dest = scratch("existing", "out.html");
    let folder = dest.with_file_name("out_files");
    std::fs::create_dir_all(&folder).expect("folder");
    std::fs::write(folder.join("notes.txt"), b"mine").expect("write");

    let _ = export_to("report.pdf", &dest).await;
    assert_eq!(std::fs::read(folder.join("notes.txt")).expect("still there"), b"mine");
    assert!(folder.join("image-001.png").is_file());
    cleanup(&dest);
}

#[tokio::test]
async fn ruled_tables_become_table_markup() {
    // Three pages, three ways of drawing the same 4 x 4 grid — including under
    // a transform — as in the Word suite.
    let (html, summary, dest) = export("table.pdf").await;
    assert_eq!(summary.tables, 3);
    assert_eq!(html.matches("<table>").count(), 3);
    assert_eq!(html.matches("<tr>").count(), 12, "four rows a table");
    assert_eq!(html.matches("<td>").count(), 48, "four cells a row");

    let first = html.split("<table>").nth(1).expect("a table");
    let header = first.split("<tr>").nth(1).expect("a row");
    // The fixture sets its header row in bold, and that survives into the cells.
    for cell in ["Region", "Revenue", "Change", "Units"] {
        assert!(header.contains(&format!("<td><p><strong>{cell}</strong></p></td>")), "header cell {cell} missing: {header}");
    }
    let last = first.split("<tr>").nth(4).expect("the fourth row");
    for cell in ["Overseas", "77,250", "+31%", "310"] {
        assert!(last.contains(&format!("<td><p>{cell}</p></td>")), "cell {cell} missing: {last}");
    }
    let third = html.split("<table>").nth(3).expect("the transformed table");
    assert!(third.contains("298,500") && third.contains("Overseas"), "the transformed grid lost its text");
    cleanup(&dest);
}

#[tokio::test]
async fn text_comes_out_in_reading_order() {
    // The fixture draws its two columns interleaved; a reader takes the left
    // column first.
    let (html, _, dest) = export("two-column.pdf").await;
    let at = |needle: &str| html.find(needle).unwrap_or_else(|| panic!("missing {needle}"));
    assert!(at("Alpha five") < at("Beta one"), "the columns were interleaved");
    cleanup(&dest);
}

#[tokio::test]
async fn unicode_survives_the_trip() {
    let (html, _, dest) = export("unicode-text.pdf").await;
    assert!(html.contains('\u{0414}') || html.contains('\u{03B1}') || html.contains('\u{00E9}'), "no non-Latin characters survived");
    cleanup(&dest);
}

#[tokio::test]
async fn the_pdf_is_not_modified() {
    let source = fixture("report.pdf");
    let before = std::fs::read(&source).expect("read");
    let (_, _, dest) = export("report.pdf").await;
    assert_eq!(std::fs::read(&source).expect("read"), before);
    cleanup(&dest);
}

#[tokio::test]
async fn an_outside_html_parser_accepts_the_output() {
    for name in ["report.pdf", "table.pdf"] {
        let (_, _, dest) = export(name).await;
        let Ok(output) = std::process::Command::new("xmllint").args(["--html", "--noout"]).arg(&dest).output() else {
            eprintln!("skipping: `xmllint` is not on PATH");
            cleanup(&dest);
            return;
        };
        let complaints = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success() && complaints.trim().is_empty(), "{name}: xmllint says:\n{complaints}");
        cleanup(&dest);
    }
}

/// Writes HTML exports into `Sample PDFs/out/p7-html/` for a human to open in
/// a browser — the check nothing here can stand in for.
/// `cargo test --test export_html -- --ignored writes_the_verification_files`
#[tokio::test]
#[ignore = "writes verification files for a human to open"]
async fn writes_the_verification_files() {
    let dir = PathBuf::from("../Sample PDFs/out/p7-html");
    std::fs::create_dir_all(&dir).expect("verification dir");
    for name in ["report.pdf", "table.pdf", "two-column.pdf", "unicode-text.pdf"] {
        let dest = dir.join(name.replace(".pdf", ".html"));
        let (_, summary) = export_to(name, &dest).await;
        println!(
            "VERIFY {}: {} paragraphs, {} headings, {} tables, {} images ({:?})",
            dest.display(),
            summary.paragraphs,
            summary.headings,
            summary.tables,
            summary.images,
            summary.images_folder
        );
    }
}
