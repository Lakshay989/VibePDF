//! Text-run extraction (P4.A1) — the read-only foundation of text editing.
//!
//! SPEC: P4-EDIT-001 — to "show an editable text box bounded by the text run's
//! bounding box" and "preserve font, size, color, and style," the frontend first
//! needs to know *where each run is* and *how it's styled*. This walks a page's
//! `PDFium` **text page-objects** (one ≈ one text-showing operator — the natural
//! edit unit) and emits a [`TextRun`] per object: its text, page-space bounding
//! box, font (name + embedded flag), size, fill colour, and text matrix.
//!
//! Unlike the cos read queries (serialize → lopdf), text decoding is `PDFium`'s
//! job, so this runs on the **live `PdfDocument`** under the shared `PDFium` lock
//! (`FX_GE` global state), exactly like [`crate::pdf::render`]. No
//! mutation: A1 is strictly read-only; editing/reflow is A3/B1.

use pdfium_render::prelude::*;

use crate::error::CommandError;
use crate::pdf::document::pdfium_lock;

/// One extracted text run, as the frontend's click-to-edit hit-test (P4.B1) and
/// the in-place editor consume it. Geometry is in PDF points (page space, origin
/// bottom-left) — the same space annotations use.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextRun {
    /// The run's Unicode text.
    pub text: String,
    /// Axis-aligned bounds `[x0, y0, x1, y1]` (over-covers rotated text — use
    /// `transform` for the true orientation).
    pub bbox: [f32; 4],
    /// Font name, subset tag (`ABCDEF+`) stripped for display.
    pub font_name: String,
    /// Whether the font is embedded in the file (drives P4.A2 fallback).
    pub embedded: bool,
    /// Scaled (on-screen) font size in points.
    pub font_size: f32,
    /// Fill colour `#rrggbb` (CMYK/gray normalized to RGB; a pattern fill → black).
    pub color: String,
    /// The text matrix `[a, b, c, d, e, f]` (rotation/skew/scale of the run).
    pub transform: [f32; 6],
}

/// SPEC: P4-EDIT-001 (infra) — extract every text run on `page` (0-based) of the
/// live document. Read-only; acquires the shared `PDFium` lock (like
/// [`crate::pdf::render::render_page`]). A getter that errors for one run is
/// skipped/defaulted, not failing the whole page.
pub fn extract_text_runs(doc: &PdfDocument, page: usize) -> Result<Vec<TextRun>, CommandError> {
    let index = i32::try_from(page)
        .map_err(|_| CommandError::InvalidInput(format!("bad page index: {page}")))?;
    let _guard = pdfium_lock()?;
    let pdf_page = doc.pages().get(index).map_err(CommandError::from)?;

    let mut runs = Vec::new();
    for object in pdf_page.objects().iter() {
        let PdfPageObject::Text(text_object) = &object else {
            continue; // images / paths are not runs (image editing is C2)
        };
        let text = text_object.text();
        if text.is_empty() {
            continue;
        }
        let Ok(bounds) = text_object.bounds() else {
            continue; // no usable geometry → can't place an editor box
        };
        let bbox = [
            bounds.left().value,
            bounds.bottom().value,
            bounds.right().value,
            bounds.top().value,
        ];
        let font = text_object.font();
        let color = text_object
            .fill_color()
            .map_or_else(|_| "#000000".to_owned(), |c| rgb_hex(c.red(), c.green(), c.blue()));
        let transform = text_object.matrix().map_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0], |m| {
            [m.a(), m.b(), m.c(), m.d(), m.e(), m.f()]
        });
        runs.push(TextRun {
            text,
            bbox,
            font_name: strip_subset_tag(&font.name()),
            embedded: font.is_embedded().unwrap_or(false),
            font_size: text_object.scaled_font_size().value,
            color,
            transform,
        });
    }
    Ok(runs)
}

/// SPEC: P4-EDIT-002 (P4.A2) — collect the **distinct** `(font name, embedded?)`
/// pairs the whole document uses, for the font-fallback report. A lightweight
/// sibling of [`extract_text_runs`]: it reads only each text object's font (no
/// text/bbox/colour), dedups as it goes, and — like A1 — runs on the live
/// `PdfDocument` under the shared `PDFium` lock. Names have the subset tag
/// stripped so `ABCDEF+Calibri` and `Calibri` collapse to one entry.
pub fn collect_document_fonts(doc: &PdfDocument) -> Result<Vec<(String, bool)>, CommandError> {
    let _guard = pdfium_lock()?;
    let mut seen = std::collections::HashSet::new();
    let mut fonts = Vec::new();
    for pdf_page in doc.pages().iter() {
        for object in pdf_page.objects().iter() {
            let PdfPageObject::Text(text_object) = &object else {
                continue;
            };
            let font = text_object.font();
            let name = strip_subset_tag(&font.name());
            if name.is_empty() {
                continue;
            }
            let embedded = font.is_embedded().unwrap_or(false);
            // Dedup on (name, embedded): a name should never appear both ways,
            // but if it did we'd rather report both than silently merge.
            if seen.insert((name.clone(), embedded)) {
                fonts.push((name, embedded));
            }
        }
    }
    Ok(fonts)
}

/// Drop a PDF subset tag — six uppercase letters + `+`, e.g. `ABCDEF+Helvetica`
/// → `Helvetica`. Subsetted embedded fonts carry one; it's noise for display.
fn strip_subset_tag(name: &str) -> String {
    if let Some((tag, rest)) = name.split_once('+') {
        if tag.len() == 6 && tag.bytes().all(|b| b.is_ascii_uppercase()) {
            return rest.to_owned();
        }
    }
    name.to_owned()
}

fn rgb_hex(r: u8, g: u8, b: u8) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::{rgb_hex, strip_subset_tag};

    #[test]
    fn strips_six_letter_subset_tag() {
        assert_eq!(strip_subset_tag("ABCDEF+Helvetica"), "Helvetica");
        assert_eq!(strip_subset_tag("BCDEEE+Arial-BoldMT"), "Arial-BoldMT");
    }

    #[test]
    fn leaves_non_subset_names_intact() {
        assert_eq!(strip_subset_tag("Helvetica"), "Helvetica");
        // Not a 6-uppercase tag → keep verbatim (real `+` in a name is rare).
        assert_eq!(strip_subset_tag("Foo+Bar"), "Foo+Bar");
        assert_eq!(strip_subset_tag("ABCDE+Helvetica"), "ABCDE+Helvetica");
    }

    #[test]
    fn formats_rgb_hex() {
        assert_eq!(rgb_hex(255, 0, 0), "#ff0000");
        assert_eq!(rgb_hex(0, 16, 255), "#0010ff");
    }
}
