//! Optical character recognition: Tesseract plus the image preparation that
//! feeds it.
//!
//! Layout mirrors `pdf/`: one module owns the foreign engine (`engine`), the
//! rest is ordinary Rust that can be tested without it.
//!
//! - [`preprocess`] — deskew / denoise / upscale, pure functions over a
//!   grayscale buffer (SPEC: P7-OCR-003).
//! - [`tessdata`] — where the bundled language data lives, and what to say when
//!   it is missing.
//! - [`engine`] — the only code that talks to Tesseract.
//!
//! What OCR produces is a *page of words with positions*, not a blob of text:
//! P7-OCR-001's searchable PDF needs each word's box to place invisible text
//! over the picture of it. So [`OcrWord`] is the unit, and page text is derived
//! from it rather than fetched separately.

pub mod engine;
pub mod preprocess;
pub mod tessdata;

/// One recognised word and where it sits in the image that was OCR'd.
#[derive(Debug, Clone, PartialEq)]
pub struct OcrWord {
    pub text: String,
    /// Tesseract's confidence, 0.0–1.0 (it reports 0–100).
    pub confidence: f32,
    /// Pixel box in the *preprocessed* image: `[left, top, right, bottom]`,
    /// origin top-left, y growing downwards — the convention Tesseract and
    /// image buffers share. Mapping back to PDF user space happens in P7.A2,
    /// where the page geometry is known.
    pub rect: [f32; 4],
}

/// Everything recognised on one page image.
#[derive(Debug, Clone, PartialEq)]
pub struct OcrPage {
    pub words: Vec<OcrWord>,
    /// Size of the image the boxes refer to.
    pub width: u32,
    pub height: u32,
}

impl OcrPage {
    /// The page's words joined with spaces, in Tesseract's reading order.
    #[must_use]
    pub fn text(&self) -> String {
        self.words
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Words at or above `confidence`, for callers that would rather drop a
    /// doubtful word than write it into a document.
    #[must_use]
    pub fn confident_words(&self, confidence: f32) -> Vec<&OcrWord> {
        self.words
            .iter()
            .filter(|w| w.confidence >= confidence)
            .collect()
    }
}
