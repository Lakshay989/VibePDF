//! SPEC: P7-OCR-001 — the only module that talks to Tesseract.
//!
//! Isolated the way `pdf::document` isolates `PDFium`: one type owns the foreign
//! handle, everything it returns is plain Rust, and no other module knows which
//! binding crate is underneath. If the binding is ever swapped (see
//! `docs/03_TECH_STACK.md` on why this one was chosen), this file is the blast
//! radius.
//!
//! Results come back as TSV rather than plain text, because a word's box is
//! what P7.A2 needs to place invisible text over the picture of it. Tesseract's
//! own `get_utf8_text` throws that away.

use tesseract_rs::TesseractAPI;

use crate::error::CommandError;
use crate::ocr::preprocess::GrayImage;
use crate::ocr::{tessdata, OcrPage, OcrWord};

/// An initialised Tesseract, bound to one language.
///
/// Creating it loads a language model (~4 MB), so callers should keep one
/// around for a batch of pages rather than per page. The underlying handle
/// guards itself with a mutex, so a shared engine is safe but serialises.
pub struct OcrEngine {
    api: TesseractAPI,
    language: String,
}

impl OcrEngine {
    /// Load `language` (e.g. `"eng"`) from the bundled data directory.
    ///
    /// # Errors
    /// [`CommandError::NotFound`] when the language data is missing —
    /// see [`tessdata::directory_for`] — and [`CommandError::Internal`] when
    /// Tesseract itself refuses to initialise.
    pub fn new(language: &str) -> Result<Self, CommandError> {
        let datapath = tessdata::directory_for(language)?;
        let api = TesseractAPI::new();
        api.init(&datapath, language).map_err(|e| {
            CommandError::Internal(format!(
                "could not start the OCR engine for \"{language}\" from {}: {e}",
                datapath.display()
            ))
        })?;
        Ok(Self {
            api,
            language: language.to_owned(),
        })
    }

    #[must_use]
    pub fn language(&self) -> &str {
        &self.language
    }

    /// Recognise one page image. The image is used as given: run it through
    /// [`crate::ocr::preprocess`] first (SPEC: P7-OCR-003).
    ///
    /// # Errors
    /// [`CommandError::InvalidInput`] for an empty image, and
    /// [`CommandError::Internal`] when Tesseract fails to recognise or returns
    /// output that can't be parsed.
    pub fn recognize(&self, image: &GrayImage) -> Result<OcrPage, CommandError> {
        if image.width == 0 || image.height == 0 {
            return Err(CommandError::InvalidInput(
                "cannot run OCR on an empty image".into(),
            ));
        }
        let width = i32::try_from(image.width)
            .map_err(|_| CommandError::InvalidInput("image is too wide for OCR".into()))?;
        let height = i32::try_from(image.height)
            .map_err(|_| CommandError::InvalidInput("image is too tall for OCR".into()))?;

        // 8-bit grayscale: one byte per pixel, rows packed with no padding.
        self.api
            .set_image(&image.pixels, width, height, 1, width)
            .map_err(|e| CommandError::Internal(format!("OCR could not read the image: {e}")))?;
        let tsv = self
            .api
            .get_tsv_text(0)
            .map_err(|e| CommandError::Internal(format!("OCR failed: {e}")))?;

        Ok(OcrPage {
            words: parse_tsv_words(&tsv),
            width: image.width,
            height: image.height,
        })
    }
}

/// Tesseract's TSV: one row per layout element, tab-separated, with a header
/// line. Columns 0..11 are level, page, block, paragraph, line, word, left,
/// top, width, height, confidence, text.
///
/// Only level 5 (word) rows carry text. Rows whose confidence is negative are
/// layout elements rather than recognised words, and rows whose text is blank
/// are spacing; both are dropped, so an empty page yields no words rather than
/// a list of empty strings.
fn parse_tsv_words(tsv: &str) -> Vec<OcrWord> {
    const WORD_LEVEL: &str = "5";
    let mut words = Vec::new();
    for line in tsv.lines().skip(1) {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 12 || cols[0] != WORD_LEVEL {
            continue;
        }
        let text = cols[11].trim();
        if text.is_empty() {
            continue;
        }
        let (Ok(left), Ok(top), Ok(width), Ok(height), Ok(conf)) = (
            cols[6].parse::<f32>(),
            cols[7].parse::<f32>(),
            cols[8].parse::<f32>(),
            cols[9].parse::<f32>(),
            cols[10].parse::<f32>(),
        ) else {
            continue;
        };
        if conf < 0.0 {
            continue;
        }
        words.push(OcrWord {
            text: text.to_owned(),
            confidence: conf / 100.0,
            rect: [left, top, left + width, top + height],
        });
    }
    words
}

#[allow(clippy::expect_used, clippy::unwrap_used)]
#[cfg(test)]
mod tsv_tests {
    use super::parse_tsv_words;

    const HEADER: &str = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext";

    #[test]
    fn reads_words_with_boxes_and_confidence() {
        let tsv = format!(
            "{HEADER}\n\
             1\t1\t0\t0\t0\t0\t0\t0\t600\t800\t-1\t\n\
             5\t1\t1\t1\t1\t1\t30\t40\t50\t12\t96.5\tHello\n\
             5\t1\t1\t1\t1\t2\t90\t40\t70\t12\t88\tVibePDF\n"
        );
        let words = parse_tsv_words(&tsv);
        assert_eq!(words.len(), 2, "the page-level row must not become a word");
        assert_eq!(words[0].text, "Hello");
        // Compared elementwise: clippy rightly objects to `==` on float arrays.
        for (got, want) in words[0].rect.iter().zip([30.0, 40.0, 80.0, 52.0]) {
            assert!((got - want).abs() < 1e-6, "box was {:?}", words[0].rect);
        }
        assert!((words[0].confidence - 0.965).abs() < 1e-6);
        assert_eq!(words[1].text, "VibePDF");
    }

    #[test]
    fn drops_blank_and_unrecognised_rows() {
        let tsv = format!(
            "{HEADER}\n\
             5\t1\t1\t1\t1\t1\t0\t0\t10\t10\t-1\tghost\n\
             5\t1\t1\t1\t1\t2\t0\t0\t10\t10\t95\t \n\
             5\t1\t1\t1\t1\t3\tx\ty\t10\t10\t95\tbroken\n\
             4\t1\t1\t1\t1\t0\t0\t0\t10\t10\t95\tline\n"
        );
        assert!(parse_tsv_words(&tsv).is_empty());
    }
}
