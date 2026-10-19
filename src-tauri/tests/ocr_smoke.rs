//! SPEC: P7-OCR-001, P7-OCR-003 — the OCR engine reads a page that contains
//! no text, only a picture of text.
//!
//! `tests/fixtures/basic/scan.pdf` is that page: its generator rasterises real
//! text at 150 DPI and embeds the raster, so nothing in the file is selectable
//! and OCR is the only way to the words. 150 DPI is below the spec's 300 DPI
//! floor on purpose, so the upscale stage runs here too.
//!
//! Accuracy is a property of Tesseract, not of this code, so the assertions
//! are what our wiring guarantees: the expected words come back, each with a
//! box inside the page, and pure noise does not produce confident words.

use std::path::PathBuf;

use vibepdf_lib::error::CommandError;
use vibepdf_lib::ocr::engine::OcrEngine;
use vibepdf_lib::ocr::preprocess::{preprocess, GrayImage, PreprocessOptions};
use vibepdf_lib::ocr::{tessdata, OcrPage};
use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::render::ImageFormat;

/// What tests/fixtures/basic/generate-scan.py drew on the page.
const EXPECTED_WORDS: &[&str] = &[
    // Heading, 24 pt.
    "VibePDF", "scanned", "page", "fixture",
    // Body, 9 pt — the size at which a careless preprocessing stage shows up.
    "quick", "brown", "fox", "jumps", "lazy", "dog", "Invoice", "number", "March", "thirty",
];
const SCAN_DPI: u32 = 150;

fn fixture(name: &str) -> PathBuf {
    // Test runs with CWD = src-tauri/.
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

/// Render page 1 of a fixture the way P7.A2 will: PDFium raster → grayscale →
/// preprocessing → OCR.
async fn ocr_fixture(name: &str, options: PreprocessOptions) -> OcrPage {
    let handle = DocumentActorHandle::spawn(None, uuid::Uuid::new_v4(), fixture(name), None)
        .expect("opens the fixture");
    let rendered = handle
        .render_page(0, SCAN_DPI as f32, ImageFormat::Rgba8)
        .await
        .expect("renders page 1");
    let gray = GrayImage::from_rgba(rendered.width, rendered.height, &rendered.bytes)
        .expect("render is RGBA");
    let prepared = preprocess(&gray, SCAN_DPI, options).expect("preprocess");
    OcrEngine::new("eng")
        .expect("starts the OCR engine")
        .recognize(&prepared)
        .expect("recognises the page")
}

#[tokio::test]
async fn reads_the_words_on_a_scanned_page() {
    let page = ocr_fixture("scan.pdf", PreprocessOptions::default()).await;
    let text = page.text().to_lowercase();
    let missing: Vec<&str> = EXPECTED_WORDS
        .iter()
        .copied()
        .filter(|w| !text.contains(&w.to_lowercase()))
        .collect();
    assert!(
        missing.is_empty(),
        "OCR missed {missing:?}; it read: {}",
        page.text()
    );
}

#[tokio::test]
async fn reports_a_box_for_each_word_inside_the_page() {
    // P7.A2 places invisible text using these boxes, so a word without a
    // plausible box is as bad as a word that was never read.
    let page = ocr_fixture("scan.pdf", PreprocessOptions::default()).await;
    assert!(page.words.len() >= EXPECTED_WORDS.len(), "too few words");

    #[allow(clippy::cast_precision_loss)]
    let (w, h) = (page.width as f32, page.height as f32);
    for word in &page.words {
        let [left, top, right, bottom] = word.rect;
        assert!(
            left >= 0.0 && top >= 0.0 && right <= w && bottom <= h,
            "{:?} has a box outside the {w}x{h} page: {:?}",
            word.text,
            word.rect
        );
        assert!(
            right > left && bottom > top,
            "{:?} has an empty box: {:?}",
            word.text,
            word.rect
        );
    }

    // The fixture has four lines; boxes must reflect that rather than all
    // collapsing onto one baseline.
    #[allow(clippy::cast_possible_truncation)]
    let mut tops: Vec<i32> = page
        .words
        .iter()
        .map(|word| (word.rect[1] / 40.0).round() as i32)
        .collect();
    tops.sort_unstable();
    tops.dedup();
    assert!(tops.len() >= 4, "expected 4 lines of text, boxes gave {tops:?}");
}

#[tokio::test]
async fn preprocessing_is_optional_and_the_page_still_reads() {
    // P7-OCR-003 says preprocessing is configurable; switching it off must not
    // break the pipeline (this page is clean and straight, so it survives).
    let page = ocr_fixture(
        "scan.pdf",
        PreprocessOptions {
            deskew: false,
            denoise: false,
            min_dpi: 0,
        },
    )
    .await;
    assert!(
        page.text().to_lowercase().contains("quick"),
        "raw page read as: {}",
        page.text()
    );
}

#[test]
fn a_page_of_noise_yields_no_confident_words() {
    // Guards the opposite failure from the tests above: an engine that
    // "reads" anything would pass those and still be useless.
    let mut pixels = vec![255u8; 600 * 400];
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    for pixel in &mut pixels {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        #[allow(clippy::cast_possible_truncation)]
        let byte = (seed >> 24) as u8;
        *pixel = byte;
    }
    let noise = GrayImage::new(600, 400, pixels).expect("size matches");
    let page = OcrEngine::new("eng")
        .expect("starts the OCR engine")
        .recognize(&noise)
        .expect("recognises (or declines to) the noise");
    let confident: Vec<&str> = page
        .confident_words(0.8)
        .into_iter()
        .map(|w| w.text.as_str())
        .filter(|w| w.chars().count() >= 3)
        .collect();
    assert!(
        confident.is_empty(),
        "noise produced confident words: {confident:?}"
    );
}

#[test]
fn missing_language_data_says_how_to_get_it() {
    let err = tessdata::directory_for("zz").expect_err("no such language is installed");
    let CommandError::NotFound(message) = err else {
        panic!("expected NotFound, got {err:?}");
    };
    assert!(
        message.contains("fetch-tessdata"),
        "the error must name the fix, was: {message}"
    );
}

#[test]
fn english_is_installed_for_the_suite() {
    assert!(
        tessdata::installed_languages().contains(&"eng".to_owned()),
        "run `npm run fetch-tessdata`; found {:?}",
        tessdata::installed_languages()
    );
}
