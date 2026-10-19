//! SPEC: P7-OCR-004 — exporting a document as Word.
//!
//! Every test here **unzips the output and reads the XML**. A `.docx` that is
//! the right size and opens as an archive can still be a file Word refuses, so
//! "a file was written" is not an assertion worth making.
//!
//! The archive is also checked with `unzip -t` — an implementation we did not
//! write — in `the_archive_passes_an_outside_checker`, which skips loudly when
//! `unzip` is absent. The final word belongs to Word itself, in the acceptance
//! pass; nothing here can stand in for that.

use std::path::{Path, PathBuf};

use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::export_docx::DocxExportSummary;
use vibepdf_lib::pdf::ooxml::{entry_names, read_entry};

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vibepdf-docx-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir.join("out.docx")
}

/// Export a fixture and hand back the archive bytes plus the summary.
async fn export(name: &str) -> (Vec<u8>, DocxExportSummary, PathBuf) {
    let dest = scratch(name);
    let handle = DocumentActorHandle::spawn(None, uuid::Uuid::new_v4(), fixture(name), None)
        .expect("opens");
    let summary = handle
        .export_docx(vec![], dest.clone())
        .await
        .expect("export succeeds");
    let bytes = std::fs::read(&dest).expect("the file exists");
    (bytes, summary, dest)
}

fn document_xml(archive: &[u8]) -> String {
    String::from_utf8(read_entry(archive, "word/document.xml").expect("document.xml"))
        .expect("document.xml is UTF-8")
}

fn cleanup(dest: &Path) {
    if let Some(parent) = dest.parent() {
        let _ = std::fs::remove_dir_all(parent);
    }
}

#[tokio::test]
async fn the_archive_has_the_parts_word_requires() {
    // Word reports "the file is corrupt" for any of these being absent, without
    // saying which — so they are asserted by name.
    let (bytes, _, dest) = export("report.pdf").await;
    let names = entry_names(&bytes).expect("entry names");
    for required in [
        "[Content_Types].xml",
        "_rels/.rels",
        "word/document.xml",
        "word/styles.xml",
        "word/_rels/document.xml.rels",
    ] {
        assert!(names.iter().any(|n| n == required), "missing {required} in {names:?}");
    }
    cleanup(&dest);
}

#[tokio::test]
async fn the_archive_passes_an_outside_checker() {
    // A reader we wrote validating a writer we wrote proves nothing about the
    // format. `unzip -t` is an implementation we did not write.
    let (_, _, dest) = export("report.pdf").await;
    let Ok(output) = std::process::Command::new("unzip").arg("-t").arg(&dest).output() else {
        eprintln!("skipping: `unzip` is not on PATH");
        cleanup(&dest);
        return;
    };
    assert!(
        output.status.success(),
        "unzip -t rejected the archive:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    cleanup(&dest);
}

#[tokio::test]
async fn headings_are_headings() {
    // SPEC: P7-OCR-004. The fixture sets 20/14/11 pt over 9 pt body, so the
    // three levels must be told apart, not merely detected.
    let (bytes, summary, dest) = export("report.pdf").await;
    let xml = document_xml(&bytes);
    assert!(xml.contains(r#"<w:pStyle w:val="Heading1"/>"#), "no Heading1");
    assert!(xml.contains(r#"<w:pStyle w:val="Heading2"/>"#), "no Heading2");
    assert!(xml.contains(r#"<w:pStyle w:val="Heading3"/>"#), "no Heading3");
    assert_eq!(summary.headings, 4, "expected four headings in the fixture");

    // And the biggest line is the one that got Heading1.
    let heading1_at = xml.find(r#"<w:pStyle w:val="Heading1"/>"#).expect("Heading1");
    let title_at = xml.find("Quarterly Report").expect("the title");
    assert!(heading1_at < title_at && title_at - heading1_at < 200);
    cleanup(&dest);
}

#[tokio::test]
async fn bold_and_italic_survive() {
    // SPEC: P7-OCR-004's "basic formatting".
    let (bytes, _, dest) = export("report.pdf").await;
    let xml = document_xml(&bytes);
    let run_of = |needle: &str| -> String {
        let at = xml.find(needle).expect(needle);
        let start = xml[..at].rfind("<w:r>").expect("run start");
        xml[start..at].to_owned()
    };
    assert!(run_of("This sentence is emphasised.").contains("<w:i/>"), "italic lost");
    assert!(run_of("This sentence is strong.").contains("<w:b/>"), "bold lost");
    // And a plain body run is not marked up.
    let plain = run_of("The quarter closed ahead of plan");
    assert!(!plain.contains("<w:b/>") && !plain.contains("<w:i/>"), "plain text was marked up");
    cleanup(&dest);
}

#[tokio::test]
async fn an_image_lands_in_the_archive_and_is_referenced() {
    // SPEC: P7-OCR-004's "images". Three things have to agree: the part, the
    // relationship, and the drawing that points at it. Word fails on any one.
    let (bytes, summary, dest) = export("report.pdf").await;
    assert_eq!(summary.images, 1);

    let png = read_entry(&bytes, "word/media/image1.png").expect("the image part");
    assert_eq!(&png[..4], b"\x89PNG", "the media part is not a PNG");

    let rels = String::from_utf8(read_entry(&bytes, "word/_rels/document.xml.rels").unwrap())
        .expect("rels is UTF-8");
    assert!(rels.contains(r#"Target="media/image1.png""#), "no relationship to the image");
    assert!(rels.contains(r#"Id="rId2""#));

    let xml = document_xml(&bytes);
    assert!(xml.contains(r#"r:embed="rId2""#), "the drawing does not reference rId2");
    // Sized in EMU from the drawn size: 128 x 96 pt -> 1625600 x 1219200.
    assert!(xml.contains(r#"cx="1625600""#), "image width is not the drawn width");
    assert!(xml.contains(r#"cy="1219200""#), "image height is not the drawn height");

    let types = String::from_utf8(read_entry(&bytes, "[Content_Types].xml").unwrap()).unwrap();
    assert!(types.contains(r#"Extension="png""#), "png has no content type");
    cleanup(&dest);
}

#[tokio::test]
async fn xml_special_characters_are_escaped() {
    // The fixture contains `&`, `<` and `"` for exactly this. An unescaped
    // ampersand produces a file every reader refuses, and the text comes from
    // whatever document the user happened to open.
    let (bytes, _, dest) = export("report.pdf").await;
    let xml = document_xml(&bytes);
    assert!(xml.contains("Revenue &amp; Costs"), "ampersand not escaped");
    assert!(xml.contains("&lt;5%"), "less-than not escaped");
    assert!(xml.contains("&quot;critical&quot;"), "quote not escaped");
    // The raw characters must not appear inside a text node.
    for piece in xml.split("<w:t xml:space=\"preserve\">").skip(1) {
        let text = piece.split("</w:t>").next().unwrap_or("");
        assert!(!text.contains('&') || text.contains("&amp;") || text.contains("&lt;")
                || text.contains("&quot;"), "raw ampersand in {text:?}");
    }
    cleanup(&dest);
}

#[tokio::test]
async fn text_comes_out_in_reading_order() {
    // SPEC: P7-OCR-004 inherits P7-OCR-006's ordering: the fixture draws its
    // columns interleaved, so content-stream order would give "Alpha one Beta
    // one". A person reads the left column, then the right.
    let (bytes, _, dest) = export("two-column.pdf").await;
    let xml = document_xml(&bytes);
    let at = |needle: &str| xml.find(needle).unwrap_or_else(|| panic!("missing {needle}"));
    assert!(at("Alpha five") < at("Beta one"), "the columns were interleaved");
    cleanup(&dest);
}

#[tokio::test]
async fn unicode_survives_the_trip() {
    // The archive is UTF-8 throughout; a lossy path turns these into `?`.
    let (bytes, _, dest) = export("unicode-text.pdf").await;
    let xml = document_xml(&bytes);
    assert!(xml.contains('\u{0414}') || xml.contains('\u{03B1}') || xml.contains('\u{00E9}'),
            "no non-Latin characters survived");
    cleanup(&dest);
}

#[tokio::test]
async fn a_scan_exports_an_empty_document_rather_than_failing() {
    // A page that is a picture of text has no text to convert. That is a fact
    // about the document, not an error — the caller tells the user to run OCR.
    let (bytes, summary, dest) = export("scan.pdf").await;
    assert_eq!(summary.paragraphs, 0, "invented text from a scan");
    assert_eq!(summary.images, 1, "the page image should still come through");
    let xml = document_xml(&bytes);
    assert!(xml.contains("<w:body>"), "not a document");
    cleanup(&dest);
}

#[tokio::test]
async fn a_document_of_one_size_gets_no_headings() {
    // There is no heading flag in a PDF, only type larger than its neighbours.
    // With one size there is no such thing, and inventing headings from nothing
    // would be worse than flat text.
    let (bytes, summary, dest) = export("two-column.pdf").await;
    assert_eq!(summary.headings, 0, "invented headings in a single-size document");
    assert!(!document_xml(&bytes).contains("<w:pStyle"), "applied a style anyway");
    cleanup(&dest);
}

/// Writes a `.docx` into `Sample PDFs/verify-docx/` so a human can open it in
/// Word — the one check nothing here can stand in for.
///
/// Ignored: it writes outside the test scratch directory.
/// `cargo test --test export_docx -- --ignored writes_the_verification_file`
#[tokio::test]
#[ignore = "writes a verification file for a human to open"]
async fn writes_the_verification_file() {
    let dir = PathBuf::from("../Sample PDFs/out/p7-docx");
    std::fs::create_dir_all(&dir).expect("verification dir");
    for name in ["report.pdf", "table.pdf", "two-column.pdf", "unicode-text.pdf"] {
        let dest = dir.join(name.replace(".pdf", ".docx"));
        let handle = DocumentActorHandle::spawn(None, uuid::Uuid::new_v4(), fixture(name), None)
            .expect("opens");
        let summary = handle
            .export_docx(vec![], dest.clone())
            .await
            .expect("export succeeds");
        println!(
            "VERIFY {}: {} paragraphs, {} headings, {} images, {} bytes",
            dest.display(),
            summary.paragraphs,
            summary.headings,
            summary.images,
            summary.bytes
        );
    }
}
