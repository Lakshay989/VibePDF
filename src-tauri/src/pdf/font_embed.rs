//! Font embedding (P4.HF5 / `FABLE_REVIEW` 3.2 stage-2) — render text the built-in
//! base-14 fonts can't, by embedding a system TrueType font through `PDFium`.
//!
//! `PDFium` already ships a full font engine; `load_true_type_from_bytes` embeds a
//! TTF as a CIDFontType2/Type0 with a `/ToUnicode` map — so the *rendering* side
//! needs **no Rust font-parsing dependency** for encoding. `PDFium` does **not**
//! subset, though (it embeds the whole face), so [`subset_font`] first reduces the
//! font to just the glyphs the runs use, via the lightweight `subsetter` +
//! `ttf-parser` crates (P4.HF6). Callers branch here only when
//! [`crate::pdf::cos::winansi_fits`] is false — `WinAnsi` text keeps the cheap
//! base-14 lopdf path, so a plain "Page 1" footer never pays for an embedded font.
//!
//! The write chassis mirrors [`crate::pdf::reflow`]: load under the global `PDFium`
//! lock, mutate each page under **Manual** content regeneration, `regenerate_content`
//! once per page, then `save_to_bytes`. Bytes → bytes, verified by the actor's reload.
//!
//! [`embed_runs`] is a dumb placement primitive: it draws each run at the exact
//! text-space → page-space `matrix` the caller computed (position + rotation), so
//! the same rotation/CropBox math the lopdf writers already use ([`visual_transform`])
//! carries straight over. Alignment width is the caller's problem (see the
//! header/footer note on estimate-based centring).
//!
//! [`visual_transform`]: crate::pdf::cos::visual_transform

use std::collections::{BTreeMap, BTreeSet};

use pdfium_render::prelude::*;

use crate::error::CommandError;
use crate::pdf::document::{pdfium, pdfium_lock};

/// Subset `font_bytes` down to only the glyphs needed to render `texts`, so the
/// embedded `/FontFile2` carries a handful of glyphs instead of the whole face
/// (`FABLE_REVIEW` 3.2 stage-2 size fix). `PDFium` won't subset for us, so we do it
/// up front with `subsetter` — its PDF profile keeps original glyph-ids and the
/// `cmap`, so `PDFium`'s Unicode→GID lookup still resolves on the subset.
///
/// **Correctness over size:** if the face can't be parsed or subsetted (an
/// unusual container, a `.ttc`, a subsetter edge case), this returns the full
/// font unchanged — a bloated-but-correct embed, never a hard failure.
fn subset_font(font_bytes: &[u8], texts: &[&str]) -> Vec<u8> {
    let Ok(face) = ttf_parser::Face::parse(font_bytes, 0) else {
        return font_bytes.to_vec();
    };
    // `.notdef` (gid 0) plus every covered codepoint's glyph. Uncovered chars are
    // simply skipped — that's the existing coverage gap, not this step's concern.
    let mut gids: BTreeSet<u16> = BTreeSet::new();
    gids.insert(0);
    for text in texts {
        for ch in text.chars() {
            if let Some(gid) = face.glyph_index(ch) {
                gids.insert(gid.0);
            }
        }
    }
    let gids: Vec<u16> = gids.into_iter().collect();
    subsetter::subset(font_bytes, 0, subsetter::Profile::pdf(&gids))
        .unwrap_or_else(|_| font_bytes.to_vec())
}

/// One positioned run of embedded Unicode text.
pub(crate) struct EmbedRun {
    /// 0-based page index.
    pub page: usize,
    /// The text to render (arbitrary Unicode the font covers).
    pub text: String,
    /// Font size in points.
    pub size: f32,
    /// Fill colour, RGB in `0.0..=1.0`.
    pub color: (f32, f32, f32),
    /// Fill opacity in `0.0..=1.0` (`PDFium` forwards it as the object's fill
    /// alpha). Header/footer passes `1.0`; watermark passes the user's opacity.
    pub opacity: f32,
    /// Text-space → page-space transform `[a b c d e f]` (position + rotation),
    /// applied verbatim as the text object's matrix.
    pub matrix: [f32; 6],
    /// Draw *under* the page's existing content (inserted at z-index 0) instead
    /// of on top. Used by a "behind" watermark.
    pub behind: bool,
}

/// Embed `runs` into `bytes` using `font_bytes` (a TrueType program that must
/// cover every run's glyphs), returning the new document bytes. The font is
/// loaded once and shared across all runs; pages are touched once each.
pub(crate) fn embed_runs(
    bytes: &[u8],
    font_bytes: &[u8],
    runs: &[EmbedRun],
) -> Result<Vec<u8>, CommandError> {
    if runs.is_empty() {
        return Ok(bytes.to_vec());
    }

    // Subset the face to just the glyphs these runs use *before* handing it to
    // PDFium — PDFium embeds whatever font it's given without subsetting, so this
    // is what keeps an embedded run from bloating the file by the whole font
    // (`FABLE_REVIEW` 3.2 stage-2 size fix). Parsing is pure Rust, no lock needed.
    let texts: Vec<&str> = runs.iter().map(|run| run.text.as_str()).collect();
    let subset = subset_font(font_bytes, &texts);

    let _guard = pdfium_lock()?;
    let mut doc = pdfium()?
        .load_pdf_from_byte_vec(bytes.to_vec(), None)
        .map_err(CommandError::from)?;

    // Load the font once (the mutable borrow ends with this statement; the token
    // is an owned handle). `is_cid_font = true` addresses glyphs beyond the 8-bit
    // range (CJK, Indic, …) via a 16-bit CID keyspace.
    let token = doc
        .fonts_mut()
        .load_true_type_from_bytes(&subset, true)
        .map_err(CommandError::from)?;

    place_and_save(doc, token, runs)
}

/// Place every run (grouped per page, one regeneration each) and serialize.
fn place_and_save(
    doc: PdfDocument<'_>,
    token: PdfFontToken,
    runs: &[EmbedRun],
) -> Result<Vec<u8>, CommandError> {
    let mut by_page: BTreeMap<usize, Vec<&EmbedRun>> = BTreeMap::new();
    for run in runs {
        by_page.entry(run.page).or_default().push(run);
    }

    for (page, page_runs) in by_page {
        let page_index = i32::try_from(page)
            .map_err(|_| CommandError::InvalidInput(format!("bad page index: {page}")))?;
        // Stage under Manual regeneration and commit once per page (see reflow.rs):
        // adding objects mutates handles but doesn't flag the page, so without an
        // explicit `regenerate_content` the runs are lost on save.
        let mut pdf_page = doc.pages().get(page_index).map_err(CommandError::from)?;
        pdf_page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
        for run in page_runs {
            place_run(&doc, &mut pdf_page, token, run)?;
        }
        pdf_page.regenerate_content().map_err(CommandError::from)?;
    }

    let out = doc.save_to_bytes().map_err(CommandError::from)?;
    drop(doc); // FPDF_CloseDocument is FFI; drop under the still-held lock.
    Ok(out)
}

/// Build one text object (coloured + opacity + matrix carrying position/rotation)
/// and add it to the page — appended on top, or inserted at z-index 0 for a
/// `behind` run. The matrix override works because a fresh text object starts at
/// identity.
fn place_run<'a>(
    doc: &PdfDocument<'a>,
    page: &mut PdfPage<'a>,
    token: PdfFontToken,
    run: &EmbedRun,
) -> Result<(), CommandError> {
    let (r, g, b) = run.color;
    let color = PdfColor::new(to_u8(r), to_u8(g), to_u8(b), to_u8(run.opacity));
    let [ma, mb, mc, md, me, mf] = run.matrix;
    let matrix = PdfMatrix::new(ma, mb, mc, md, me, mf);

    if run.behind {
        // Create detached, then insert under all existing content (index 0).
        let mut object =
            PdfPageTextObject::new(doc, &run.text, token, PdfPoints::new(run.size))
                .map_err(CommandError::from)?;
        object.set_fill_color(color).map_err(CommandError::from)?;
        object.apply_matrix(matrix).map_err(CommandError::from)?;
        page.objects_mut()
            .insert_object_at_index(0, PdfPageObject::Text(object))
            .map_err(CommandError::from)?;
    } else {
        let mut object = page
            .objects_mut()
            .create_text_object(
                PdfPoints::new(0.0),
                PdfPoints::new(0.0),
                &run.text,
                token,
                PdfPoints::new(run.size),
            )
            .map_err(CommandError::from)?;
        object.set_fill_color(color).map_err(CommandError::from)?;
        object.apply_matrix(matrix).map_err(CommandError::from)?;
    }
    Ok(())
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn to_u8(channel: f32) -> u8 {
    (channel.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn hello_bytes() -> Vec<u8> {
        std::fs::read("../tests/fixtures/basic/hello.pdf").expect("read hello.pdf")
    }

    /// Concatenate every text object's text on page 0 (mirrors `text_extract`'s
    /// per-object read). Re-extraction is the strongest proof the `/ToUnicode`
    /// map round-trips.
    fn page0_text(bytes: &[u8]) -> String {
        let _guard = pdfium_lock().expect("lock");
        let doc = pdfium()
            .expect("pdfium")
            .load_pdf_from_byte_vec(bytes.to_vec(), None)
            .expect("reopen");
        let page = doc.pages().get(0).expect("page 0");
        let mut out = String::new();
        for object in page.objects().iter() {
            if let Some(t) = object.as_text_object() {
                out.push_str(&t.text());
            }
        }
        drop(doc);
        out
    }

    fn coptic_font() -> Vec<u8> {
        std::fs::read("../tests/fixtures/fonts/NotoSansCoptic-Regular.ttf")
            .expect("read committed Coptic fixture font")
    }

    /// P4.HF5 core regression (was increment 1's spike): `PDFium` embeds a loaded
    /// TrueType font and the Unicode text survives our save round-trip *and*
    /// re-extracts — proving the `/ToUnicode` map is written. If this ever fails,
    /// the whole `PDFium`-embed approach is broken.
    #[test]
    fn pdfium_embeds_truetype_and_unicode_survives_roundtrip() {
        let text = "\u{2C81}\u{2C83}\u{2C85}"; // Coptic Ⲁ Ⲃ Ⲅ — outside WinAnsi
        let runs = [EmbedRun {
            page: 0,
            text: text.to_string(),
            size: 24.0,
            color: (0.0, 0.0, 0.0),
            opacity: 1.0,
            behind: false,
            matrix: [1.0, 0.0, 0.0, 1.0, 72.0, 700.0],
        }];
        let out = embed_runs(&hello_bytes(), &coptic_font(), &runs).expect("embed");
        let extracted = page0_text(&out);
        assert!(
            extracted.contains(text),
            "embedded Coptic text must survive reopen; got: {extracted:?}",
        );
    }

    /// P4.HF6 spike / regression: subsetting shrinks the font *and* `PDFium` still
    /// renders + re-extracts the Unicode through the subset (proving `subsetter`'s
    /// PDF profile keeps a cmap `PDFium` can resolve). If this fails, Path 1 is dead
    /// and we'd fall back to building the CID font in lopdf — stop and report.
    #[test]
    fn subset_shrinks_font_and_still_embeds_unicode() {
        let full = coptic_font();
        let text = "\u{2C81}\u{2C83}\u{2C85}"; // Ⲁ Ⲃ Ⲅ
        let sub = subset_font(&full, &[text]);
        assert!(
            sub.len() < full.len(),
            "subset ({}) must be smaller than the full font ({})",
            sub.len(),
            full.len(),
        );
        let runs = [EmbedRun {
            page: 0,
            text: text.to_string(),
            size: 24.0,
            color: (0.0, 0.0, 0.0),
            opacity: 1.0,
            behind: false,
            matrix: [1.0, 0.0, 0.0, 1.0, 72.0, 700.0],
        }];
        let out = embed_runs(&hello_bytes(), &sub, &runs).expect("embed subset");
        assert!(
            page0_text(&out).contains(text),
            "subset-embedded Coptic must survive reopen; got {:?}",
            page0_text(&out),
        );
    }

    /// The base-14 (`WinAnsi`) content is untouched by embedding: the original
    /// "Hello, `VibePDF`." text object is still present after we add an embedded run.
    #[test]
    fn embedding_preserves_existing_page_text() {
        let runs = [EmbedRun {
            page: 0,
            text: "\u{2C81}".to_string(),
            size: 18.0,
            color: (0.0, 0.0, 0.0),
            opacity: 1.0,
            behind: false,
            matrix: [1.0, 0.0, 0.0, 1.0, 72.0, 500.0],
        }];
        let out = embed_runs(&hello_bytes(), &coptic_font(), &runs).expect("embed");
        assert!(page0_text(&out).contains("VibePDF"), "original page text survives");
    }

    fn embed_one(text: &str, opacity: f32, behind: bool) -> Vec<u8> {
        let runs = [EmbedRun {
            page: 0,
            text: text.to_string(),
            size: 24.0,
            color: (0.0, 0.0, 0.0),
            opacity,
            behind,
            matrix: [1.0, 0.0, 0.0, 1.0, 72.0, 650.0],
        }];
        embed_runs(&hello_bytes(), &coptic_font(), &runs).expect("embed")
    }

    /// P4.HF7: a translucent (opacity < 1) embedded run still embeds + re-extracts
    /// — the fill alpha threads through cleanly. (The *visual* opacity is a
    /// human/artifact check; here we only prove it doesn't break the pipe.)
    #[test]
    fn embedded_run_honors_opacity() {
        let text = "\u{2C81}\u{2C83}";
        assert!(
            page0_text(&embed_one(text, 0.3, false)).contains(text),
            "translucent embedded text round-trips",
        );
    }

    /// P4.HF7: `behind` inserts the run at z-index 0, so it draws *before* the
    /// page's own content; on-top appends, drawing *after*. Object iteration order
    /// is draw order, so the relative position of the two texts flips.
    #[test]
    fn behind_run_draws_under_page_content() {
        let mark = "\u{2C81}";
        let behind = page0_text(&embed_one(mark, 1.0, true));
        let on_top = page0_text(&embed_one(mark, 1.0, false));
        assert!(
            behind.find(mark) < behind.find("VibePDF"),
            "behind: embedded run precedes page content in draw order; got {behind:?}",
        );
        assert!(
            on_top.find("VibePDF") < on_top.find(mark),
            "on-top: embedded run follows page content; got {on_top:?}",
        );
    }
}
