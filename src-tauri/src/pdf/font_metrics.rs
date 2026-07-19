//! Exact Core-14 glyph metrics for base-14 text alignment and wrapping.
//!
//! The base-14 fonts are never embedded — a viewer lays them out with its own
//! standard metrics, which for a spec-compliant font *are* the Adobe AFM
//! advance widths. So to place centred/right-aligned text (or wrap it) exactly
//! where the viewer will, we need those same widths, not a flat average
//! (`cos::font_avg_em`, which biased ~0.6 em/char and drifted several points
//! per string). `FABLE_REVIEW` §3.10 (P4.HF16).
//!
//! The per-glyph tables in [`tables`] are **generated** from bundled `PDFium` by
//! `tests/gen_font_metrics.rs` (offline, no new deps — `PDFium`'s Foxit Core-14
//! substitutes are metric-compatible with Adobe's AFMs). Embedded / non-WinAnsi
//! text is unaffected: it already carries exact subset widths from the stage-2
//! embedding path.

mod tables;

use crate::pdf::cos::winansi_byte;

/// Courier and its variants are monospaced — every glyph advances 600/1000 em,
/// so they need no table.
const COURIER_ADVANCE: u16 = 600;

/// The advance-width table (/1000 em, WinAnsi-indexed) for a base-14 font name,
/// or `None` for the monospaced Courier family (use [`COURIER_ADVANCE`]).
/// Unknown names fall back to Helvetica, matching `cos::base_font`'s default.
fn table_for(base: &str) -> Option<&'static [u16; 256]> {
    match base {
        "Helvetica-Bold" => Some(&tables::HELVETICA_BOLD),
        "Helvetica-Oblique" => Some(&tables::HELVETICA_OBLIQUE),
        "Helvetica-BoldOblique" => Some(&tables::HELVETICA_BOLD_OBLIQUE),
        "Times-Roman" => Some(&tables::TIMES_ROMAN),
        "Times-Bold" => Some(&tables::TIMES_BOLD),
        "Times-Italic" => Some(&tables::TIMES_ITALIC),
        "Times-BoldItalic" => Some(&tables::TIMES_BOLD_ITALIC),
        "Courier" | "Courier-Bold" | "Courier-Oblique" | "Courier-BoldOblique" => None,
        // "Helvetica" and any unknown name.
        _ => Some(&tables::HELVETICA),
    }
}

/// Total advance width of `text` set in the base-14 font `base` at `size`
/// points. Exact for `WinAnsi` text in the standard fonts (Adobe AFM widths).
/// Characters outside `WinAnsiEncoding` — which the writers gate out before
/// reaching here — fall back to the font's average (`cos::font_avg_em`).
pub(crate) fn text_width(base: &str, text: &str, size: f32) -> f32 {
    #[allow(clippy::cast_precision_loss)]
    let width = per_mille(base, text) as f32;
    width * size / 1000.0
}

/// Advance width in 1000-unit em space (the sum of the glyph advances).
fn per_mille(base: &str, text: &str) -> u32 {
    let table = table_for(base);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let fallback = (crate::pdf::cos::font_avg_em(base) * 1000.0).round() as u32;
    text.chars()
        .map(|ch| match winansi_byte(ch) {
            Some(byte) => match table {
                Some(t) => u32::from(t[byte as usize]),
                None => u32::from(COURIER_ADVANCE),
            },
            None => fallback,
        })
        .sum()
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn proportional_fonts_diverge_per_string() {
        // The whole point of §3.10: identical char counts, very different widths.
        assert!(
            text_width("Helvetica", "WWW", 12.0) > text_width("Helvetica", "iii", 12.0),
            "'WWW' must be far wider than 'iii' in a proportional font"
        );
    }

    #[test]
    fn courier_is_monospaced() {
        assert_eq!(
            text_width("Courier", "WWW", 12.0),
            text_width("Courier", "iii", 12.0),
            "Courier advances every glyph equally"
        );
        // 3 glyphs × 600/1000 em × 10pt = 18pt.
        assert_eq!(text_width("Courier", "abc", 10.0), 18.0);
    }

    #[test]
    fn matches_known_afm_widths() {
        // Helvetica: 'A'=667, 'V'=667 → 1334/1000 em; space=278.
        assert_eq!(text_width("Helvetica", "AV", 1000.0), 1334.0);
        assert_eq!(text_width("Helvetica", " ", 1000.0), 278.0);
        // Times-Roman 'A'=722.
        assert_eq!(text_width("Times-Roman", "A", 1000.0), 722.0);
    }

    #[test]
    fn scales_linearly_with_size() {
        let a = text_width("Helvetica", "Hello", 10.0);
        let b = text_width("Helvetica", "Hello", 20.0);
        assert_eq!(b, a * 2.0);
    }

    #[test]
    fn unknown_font_falls_back_to_helvetica() {
        assert_eq!(
            text_width("Wingdings", "Hello", 12.0),
            text_width("Helvetica", "Hello", 12.0),
        );
    }
}
