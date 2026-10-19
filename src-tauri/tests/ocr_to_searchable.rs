//! SPEC: P7-OCR-001 — a scanned page becomes searchable: the words Tesseract
//! reads are written back as invisible text, positioned over the ink.
//!
//! The fixtures are pictures of text with nothing selectable in them, so
//! "before" is always empty and anything extracted afterwards came from OCR.
//!
//! **How placement is checked.** The generator drew each line at a known page
//! coordinate before rasterising, so the tests assert the OCR text lands there
//! — in PDF points, worked out from the generator rather than borrowed from the
//! code under test. The variants are what make that meaningful:
//!
//! - `scan-rotated.pdf` holds the scan sideways on a landscape page with
//!   `/Rotate 270` turning it upright, as a sheet fed in rotated comes out. It
//!   displays identically to `scan.pdf`, so the expected page coordinates are
//!   the upright ones passed through that rotation, by hand. A viewer-space
//!   mistake (mirroring, swapped axes) moves the text and fails here while
//!   leaving the upright test green.
//! - `scan-skewed.pdf` is 3 degrees off level. Recognition straightens the
//!   image, so the text layer has to be tilted back by the same angle.

use std::path::PathBuf;

use vibepdf_lib::error::CommandError;
use vibepdf_lib::ocr::preprocess::PreprocessOptions;
use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::ocr_text_layer::OcrOptions;
use vibepdf_lib::pdf::document::PROTECTED_EDIT_REFUSAL;
use vibepdf_lib::pdf::render::ImageFormat;
use vibepdf_lib::security::encrypt::{encrypt_document, DocumentPermissions, EncryptOptions};

/// Mirrors LINES and the layout loop in tests/fixtures/basic/generate-scan.py:
/// (font size, first word, baseline y in page points). x is always 72.
const LINES: &[(f32, &str, f32)] = &[
    (24.0, "VibePDF", 672.0),
    (9.0, "The", 612.0),
    (9.0, "Invoice", 589.5),
    (9.0, "Total", 567.0),
];
const LEFT_MARGIN: f32 = 72.0;
/// OCR boxes hug the glyphs and a baseline sits above a descender, so "in the
/// right place" is a few points, not a few hundredths.
const TOLERANCE: f32 = 12.0;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn open(name: &str) -> DocumentActorHandle {
    DocumentActorHandle::spawn(None, uuid::Uuid::new_v4(), fixture(name), None).expect("opens")
}

fn options() -> OcrOptions {
    OcrOptions::default()
}

/// Every word PDFium can extract from page 1, lowercased.
async fn page_text(handle: &DocumentActorHandle) -> String {
    handle
        .read_text_runs(0)
        .await
        .expect("reads text runs")
        .iter()
        .map(|run| run.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[tokio::test]
async fn a_scanned_page_starts_with_no_text_at_all() {
    // The premise of the whole step. If this ever fails, the fixture stopped
    // being a scan and every other test here proves nothing.
    let handle = open("scan.pdf");
    assert_eq!(page_text(&handle).await.trim(), "", "the fixture is not a picture-only page");
}

#[tokio::test]
async fn text_becomes_findable_on_a_scanned_page() {
    let handle = open("scan.pdf");
    let (summary, history) = handle.run_ocr(vec![0], options()).await.expect("ocr runs");

    assert_eq!(summary.pages, 1);
    assert!(summary.words >= 20, "only {} words written", summary.words);
    assert!(history.can_undo, "OCR must be undoable");

    let text = page_text(&handle).await;
    for word in ["vibepdf", "quick", "brown", "fox", "invoice", "march", "thirty"] {
        assert!(text.contains(word), "{word:?} not searchable; page reads: {text}");
    }
}

#[tokio::test]
async fn the_page_still_looks_exactly_the_same() {
    // SPEC: P7-OCR-001 — "invisible" is not a detail. Text render mode 3 lays
    // the glyphs out without painting them; get that wrong and the scan is
    // covered in black words.
    let handle = open("scan.pdf");
    let before = handle
        .render_page(0, 96.0, ImageFormat::Rgba8)
        .await
        .expect("renders before");
    handle.run_ocr(vec![0], options()).await.expect("ocr runs");
    let after = handle
        .render_page(0, 96.0, ImageFormat::Rgba8)
        .await
        .expect("renders after");

    assert_eq!(
        (before.width, before.height),
        (after.width, after.height),
        "the page changed size"
    );
    let changed = before
        .bytes
        .iter()
        .zip(after.bytes.iter())
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(changed, 0, "{changed} bytes of the rendered page changed");
}

/// The first word of each line: its text, where the reader says the run
/// starts (the text matrix's translation, in page points) and its angle.
async fn first_words(handle: &DocumentActorHandle) -> Vec<(String, (f32, f32), f32)> {
    let runs = handle.read_text_runs(0).await.expect("reads text runs");
    LINES
        .iter()
        .filter_map(|(_, word, _)| {
            runs.iter()
                .find(|run| run.text.trim().eq_ignore_ascii_case(word))
                .map(|run| {
                    let [a, b, _, _, e, f] = run.transform;
                    (run.text.trim().to_owned(), (e, f), b.atan2(a).to_degrees())
                })
        })
        .collect()
}

/// Assert the first word of every line starts where the generator drew it.
/// `expected` takes the line's baseline and returns the page-space origin, so
/// each fixture states its own geometry rather than borrowing the code's.
async fn assert_lines_start_at(fixture: &str, expected: impl Fn(f32) -> (f32, f32)) {
    let handle = open(fixture);
    handle.run_ocr(vec![0], options()).await.expect("ocr runs");
    let found = first_words(&handle).await;
    assert_eq!(
        found.len(),
        LINES.len(),
        "{fixture}: expected the first word of each line, got {:?}",
        found.iter().map(|(t, ..)| t).collect::<Vec<_>>()
    );

    for ((_, _, baseline), (text, (x, y), _)) in LINES.iter().zip(&found) {
        let (want_x, want_y) = expected(*baseline);
        assert!(
            (x - want_x).abs() <= TOLERANCE && (y - want_y).abs() <= TOLERANCE,
            "{fixture}: {text:?} starts at ({x:.1}, {y:.1}), drawn at ({want_x:.1}, {want_y:.1})"
        );
    }
}

#[tokio::test]
async fn words_land_where_the_picture_shows_them() {
    assert_lines_start_at("scan.pdf", |baseline| (LEFT_MARGIN, baseline)).await;
}

#[tokio::test]
async fn words_land_correctly_when_the_page_was_upscaled() {
    // With the defaults the page is rendered at 300 DPI and the upscale stage
    // does nothing, so the code that maps boxes back through it is never
    // exercised. Rendering at 150 forces a 2x upscale (P7-OCR-003) and the
    // words still have to land on their ink.
    let handle = open("scan.pdf");
    handle
        .run_ocr(
            vec![0],
            OcrOptions {
                dpi: 150.0,
                ..options()
            },
        )
        .await
        .expect("ocr runs");

    let found = first_words(&handle).await;
    assert_eq!(found.len(), LINES.len(), "got {:?}", found);
    for ((_, _, baseline), (text, (x, y), _)) in LINES.iter().zip(&found) {
        assert!(
            (x - LEFT_MARGIN).abs() <= TOLERANCE && (y - baseline).abs() <= TOLERANCE,
            "{text:?} starts at ({x:.1}, {y:.1}), drawn at ({LEFT_MARGIN}, {baseline})"
        );
    }
}

#[tokio::test]
async fn a_rotated_page_gets_its_text_in_the_right_place() {
    // scan-rotated.pdf holds the same scan turned on its side on a 792x612
    // page, with /Rotate 270 turning it upright for the reader — a sheet fed
    // in rotated. Displayed, it is pixel-for-pixel scan.pdf, so a word drawn
    // at visual (72, baseline) sits in page space at:
    //   page_x = visual_y            = baseline
    //   page_y = 612 - visual_x      = 540
    // (that is `visual_transform(270, [0, 0, 792, 612])` worked through by
    // hand; if the code's own mapping is wrong, this catches it).
    assert_lines_start_at("scan-rotated.pdf", |baseline| (baseline, 612.0 - LEFT_MARGIN)).await;

    // …and the text must follow the sideways ink, not lie flat on the page.
    let handle = open("scan-rotated.pdf");
    handle.run_ocr(vec![0], options()).await.expect("ocr runs");
    for (text, _, degrees) in first_words(&handle).await {
        assert!(
            (degrees.abs() - 90.0).abs() <= 2.0,
            "{text:?} sits at {degrees:.1} degrees on a sideways page"
        );
    }
}

#[tokio::test]
async fn a_skewed_page_keeps_its_text_on_the_slant() {
    // The scan is 3 degrees off level. Recognition straightens it; the text
    // layer must be tilted back, or every word drifts from its ink across the
    // page. The line origins are the rotation centre of the drawn text, so
    // they land where the upright fixture's do.
    assert_lines_start_at("scan-skewed.pdf", |baseline| (LEFT_MARGIN, baseline)).await;

    let handle = open("scan-skewed.pdf");
    handle.run_ocr(vec![0], options()).await.expect("ocr runs");
    let found = first_words(&handle).await;
    assert!(!found.is_empty(), "no words recognised on the skewed page");
    for (text, _, degrees) in found {
        assert!(
            (degrees - 3.0).abs() <= 1.5,
            "{text:?} sits at {degrees:.2} degrees, the scan is at 3"
        );
    }
}

#[tokio::test]
async fn undo_takes_the_text_layer_back_off() {
    let handle = open("scan.pdf");
    handle.run_ocr(vec![0], options()).await.expect("ocr runs");
    assert!(!page_text(&handle).await.trim().is_empty());

    handle.undo().await.expect("undo");
    assert_eq!(
        page_text(&handle).await.trim(),
        "",
        "undo left text behind"
    );
}

#[tokio::test]
async fn low_confidence_words_are_left_out() {
    // A floor of 1.0 is unreachable, so nothing should be written — and the
    // run must still report what it read rather than failing.
    let handle = open("scan.pdf");
    let (summary, _) = handle
        .run_ocr(
            vec![0],
            OcrOptions {
                min_confidence: 1.01,
                ..options()
            },
        )
        .await
        .expect("ocr runs");
    assert_eq!(summary.words, 0);
    assert!(summary.skipped > 0, "nothing was recognised at all");
    assert_eq!(page_text(&handle).await.trim(), "", "words below the floor were written");
}

#[tokio::test]
async fn preprocessing_can_be_switched_off() {
    // SPEC: P7-OCR-003 — with every stage off the boxes come straight from the
    // render, which is the simplest mapping the placement code can face.
    let handle = open("scan.pdf");
    handle
        .run_ocr(
            vec![0],
            OcrOptions {
                preprocess: PreprocessOptions {
                    deskew: false,
                    denoise: false,
                    min_dpi: 0,
                },
                ..options()
            },
        )
        .await
        .expect("ocr runs");
    assert!(page_text(&handle).await.contains("quick"));
}

#[tokio::test]
async fn a_missing_language_is_refused_before_anything_is_written() {
    let handle = open("scan.pdf");
    let err = handle
        .run_ocr(
            vec![0],
            OcrOptions {
                language: "zz".into(),
                ..options()
            },
        )
        .await
        .expect_err("no such language");
    assert!(matches!(err, CommandError::NotFound(_)), "got {err:?}");
    assert_eq!(page_text(&handle).await.trim(), "", "a failed run edited the document");
}

#[tokio::test]
async fn a_protected_document_is_refused_rather_than_damaged() {
    // SPEC: P1-VIEW-003 — the text layer is written through lopdf, which cannot
    // encrypt what it adds, so this edit is refused like every other one on a
    // protected document. Recognition happens first and is harmless; the write
    // is what stops.
    let scratch = std::env::temp_dir().join(format!("vibepdf-ocr-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&scratch).expect("scratch dir");
    let protected = scratch.join("protected-scan.pdf");
    let encrypted = encrypt_document(
        &std::fs::read(fixture("scan.pdf")).expect("read fixture"),
        &EncryptOptions {
            user_password: Some("open-me".into()),
            owner_password: None,
            permissions: DocumentPermissions::default(),
        },
    )
    .expect("encrypt");
    std::fs::write(&protected, encrypted).expect("write");

    let handle = DocumentActorHandle::spawn(
        None,
        uuid::Uuid::new_v4(),
        protected.clone(),
        Some("open-me".into()),
    )
    .expect("opens");
    let err = handle
        .run_ocr(vec![0], options())
        .await
        .expect_err("a protected document must be refused");
    match err {
        CommandError::InvalidInput(message) => assert_eq!(message, PROTECTED_EDIT_REFUSAL),
        other => panic!("expected the protected-edit refusal, got {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&scratch);
}

/// Writes `Sample PDFs/vibepdf-verify-ocr.pdf` for the cross-reader check: the
/// page must look untouched, and selecting or searching must find the text.
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn writes_verification_artifact() {
    let out = PathBuf::from("../Sample PDFs/out/p7-ocr/vibepdf-verify-ocr.pdf");
    std::fs::copy(fixture("scan.pdf"), &out).expect("copy fixture");
    let handle = DocumentActorHandle::spawn(None, uuid::Uuid::new_v4(), out.clone(), None)
        .expect("opens");
    let (summary, _) = handle.run_ocr(vec![0], options()).await.expect("ocr runs");
    handle.save(None).await.expect("save");
    drop(handle);
    let _ = std::fs::remove_file(out.with_extension("pdf.bak"));
    println!(
        "wrote {} — {} words in {} ms",
        out.display(),
        summary.words,
        summary.milliseconds
    );
}
