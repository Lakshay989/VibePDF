//! SPEC: P7-OCR-006 — exporting a document as plain text: reading order, and
//! UTF-8 that survives the trip.
//!
//! The ordering itself is unit-tested as a pure function in
//! `pdf::export_text`; these tests run it through real documents and PDFium,
//! and write real files.

use std::path::PathBuf;

use vibepdf_lib::error::CommandError;
use vibepdf_lib::pdf::actor::DocumentActorHandle;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn open(name: &str) -> DocumentActorHandle {
    DocumentActorHandle::spawn(None, uuid::Uuid::new_v4(), fixture(name), None).expect("opens")
}

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vibepdf-export-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir.join("out.txt")
}

/// Export the whole document and read the file back as UTF-8.
async fn export(name: &str, pages: Vec<i32>) -> (String, u64) {
    let dest = scratch(name);
    let handle = open(name);
    let summary = handle
        .export_text(pages, dest.clone())
        .await
        .expect("export succeeds");
    let bytes = std::fs::read(&dest).expect("the file exists");
    let text = String::from_utf8(bytes).expect("the file is valid UTF-8");
    let _ = std::fs::remove_dir_all(dest.parent().expect("parent"));
    (text, summary.characters)
}

#[tokio::test]
async fn text_comes_out_in_reading_order() {
    // SPEC: P7-OCR-006. The fixture draws its two columns interleaved, and
    // PDFium returns them that way ("Alpha one Beta one"), so concatenating
    // what PDFium gives would fail here. A person reads the left column, then
    // the right.
    let (text, _) = export("two-column.pdf", vec![]).await;
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(
        lines,
        vec![
            "Alpha one",
            "Alpha two",
            "Alpha three",
            "Alpha four",
            "Alpha five",
            "Beta one",
            "Beta two",
            "Beta three",
            "Beta four",
            "Beta five",
        ],
        "columns were not read one after the other"
    );
}

#[tokio::test]
async fn a_single_column_page_reads_straight_through() {
    let (text, characters) = export("hello.pdf", vec![]).await;
    assert_eq!(text.trim(), "Hello, VibePDF.");
    assert!(characters > 0);
}

#[tokio::test]
async fn pages_come_out_in_order() {
    let (text, _) = export("many-pages.pdf", vec![]).await;
    let at = |needle: &str| text.find(needle).unwrap_or_else(|| panic!("{needle} missing"));
    assert!(at("Page 1") < at("Page 2"));
    assert!(at("Page 2") < at("Page 10"));
    assert!(at("Page 10") < at("Page 50"));
}

#[tokio::test]
async fn only_the_pages_asked_for_are_exported() {
    let (text, _) = export("many-pages.pdf", vec![2]).await;
    assert!(text.contains("Page 3"), "expected page 3, got: {text}");
    assert!(!text.contains("Page 1\n"), "exported more than asked: {text}");
}

#[tokio::test]
async fn non_latin_text_survives_as_utf8() {
    // SPEC: P7-OCR-006 — "emit Unicode UTF-8". The fixture is Cyrillic, Greek
    // and accented Latin set in an embedded font; a broken encoding path turns
    // these into question marks or mojibake, and the read-back above would
    // already have failed if the bytes were not UTF-8.
    let (text, _) = export("unicode-text.pdf", vec![]).await;
    for expected in ["Здравствуйте", "мир", "Καλημέρα", "κόσμε", "Grüße", "naïve", "café"] {
        assert!(text.contains(expected), "{expected:?} missing from: {text}");
    }
}

#[tokio::test]
async fn a_scan_exports_nothing_until_it_has_been_read() {
    // Honest behaviour rather than an error: the page genuinely has no text.
    // The UI turns this into "use Read text… first".
    let (text, characters) = export("scan.pdf", vec![]).await;
    assert_eq!(text.trim(), "");
    assert_eq!(characters, 0);
}

#[tokio::test]
async fn a_page_out_of_range_is_refused_and_writes_nothing() {
    let dest = scratch("range");
    let handle = open("hello.pdf");
    let err = handle
        .export_text(vec![7], dest.clone())
        .await
        .expect_err("page 8 does not exist");
    assert!(matches!(err, CommandError::InvalidInput(_)), "got {err:?}");
    assert!(!dest.is_file(), "a refused export wrote a file anyway");
    let _ = std::fs::remove_dir_all(dest.parent().expect("parent"));
}

#[tokio::test]
async fn a_rotated_page_reads_in_the_order_it_is_displayed() {
    // rotated.pdf carries /Rotate 0/90/180/270, one per page. Each page has a
    // single line, so what this pins is that rotation doesn't lose the text or
    // scramble the pages — the geometry is mapped into the displayed frame.
    let (text, _) = export("rotated.pdf", vec![]).await;
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 4, "expected one line per page, got: {text}");
    // The fixture labels each page with its rotation, not its index.
    for (angle, line) in ["0", "90", "180", "270"].iter().zip(&lines) {
        assert!(
            line.contains(&format!("Rotate {angle}")),
            "the /Rotate {angle} page read as {line:?}"
        );
    }
}
