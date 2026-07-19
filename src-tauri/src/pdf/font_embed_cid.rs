//! Hand-built CID font embedding (P4.HF9 / `FABLE_REVIEW` 3.2 stage-2, `/AP` class).
//!
//! The page-content writers (HF5–HF8) embed Unicode via `PDFium` text *objects*, but
//! an annotation draws its text inside its `/AP` appearance stream — a Form
//! `XObject` with its own `/Resources` — where `PDFium`'s page-object API can't
//! reach. So for free-text / stamps we build a `Type0` / `CIDFontType2` font **by
//! hand in lopdf** (the approach printpdf / typst / pdf-writer all take) and write
//! the appearance content with Identity-H, GID-indexed hex strings.
//!
//! [`build_cid_font`] subsets the face (HF6's `subsetter`, which preserves original
//! glyph-ids), reads real advances + metrics from `ttf-parser`, and emits the
//! `/FontFile2` + `/FontDescriptor` + `CIDFontType2` + `Type0` + `/ToUnicode`
//! object graph, returning the `Type0` dict to drop into the `/AP`'s `/Resources`
//! plus an encoder ([`CidFont::encode_hex`]) and real widths ([`CidFont::width`]).

// Font-metric conversions scale i16/u16 font units into PDF's 1000-unit text
// space; the lossy numeric casts are intrinsic and bounded (glyph metrics).
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]

use std::collections::BTreeMap;
use std::fmt::Write as _;

use lopdf::{Dictionary, Document, Object, ObjectId, Stream};

use crate::error::CommandError;

/// A hand-built `Type0` CID font, plus what a content-stream writer needs to use
/// it: an Identity-H encoder and real glyph advances.
pub(crate) struct CidFont {
    /// The `Type0` font dictionary — inline it as an `/AP` `/Resources /Font` entry.
    pub font_dict: Dictionary,
    /// Char → glyph id (for Identity-H encoding). Uncovered chars map to `.notdef`.
    gid_by_char: BTreeMap<char, u16>,
    /// Glyph id → advance, scaled to the 1000-unit text space (for `/W` + wrapping).
    advance_by_gid: BTreeMap<u16, u16>,
}

impl CidFont {
    /// Encode `text` as the body of an Identity-H hex string (no `<>`): two bytes
    /// per char, the glyph id (or `0000` = `.notdef` for an uncovered char).
    pub fn encode_hex(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.chars().count() * 4);
        for ch in text.chars() {
            let gid = self.gid_by_char.get(&ch).copied().unwrap_or(0);
            let _ = write!(out, "{gid:04X}");
        }
        out
    }

    /// Rendered width of `text` at `size` points, from the font's real advances.
    #[must_use]
    pub fn width(&self, text: &str, size: f32) -> f32 {
        let units: u32 = text
            .chars()
            .map(|ch| {
                let gid = self.gid_by_char.get(&ch).copied().unwrap_or(0);
                u32::from(self.advance_by_gid.get(&gid).copied().unwrap_or(0))
            })
            .sum();
        size * units as f32 / 1000.0
    }
}

/// Build a `Type0` CID font covering `text`, adding its stream/dict objects to
/// `doc`. Returns the `Type0` dict + encoder. A face that can't be parsed is a
/// typed error (the caller has already decided this text needs embedding).
pub(crate) fn build_cid_font(
    doc: &mut Document,
    font_bytes: &[u8],
    text: &str,
) -> Result<CidFont, CommandError> {
    let face = ttf_parser::Face::parse(font_bytes, 0)
        .map_err(|_| CommandError::InvalidInput("could not parse the embedding font".into()))?;
    let scale = 1000.0 / f32::from(face.units_per_em().max(1));
    let scaled = |v: i32| -> i64 { (v as f32 * scale).round() as i64 };

    // Used chars → glyph ids (dedup), always keeping `.notdef` (gid 0).
    let mut gid_by_char: BTreeMap<char, u16> = BTreeMap::new();
    let mut gids: Vec<u16> = vec![0];
    for ch in text.chars() {
        if let Some(g) = face.glyph_index(ch) {
            gid_by_char.insert(ch, g.0);
            if !gids.contains(&g.0) {
                gids.push(g.0);
            }
        }
    }
    gids.sort_unstable();

    // Subset the face to those glyphs (preserves original gids, so Identity-H
    // codes stay valid); embed the whole face if subsetting can't run.
    let subset = subsetter::subset(font_bytes, 0, subsetter::Profile::pdf(&gids))
        .unwrap_or_else(|_| font_bytes.to_vec());

    // Real advances, scaled to 1000/em.
    let mut advance_by_gid: BTreeMap<u16, u16> = BTreeMap::new();
    for &g in &gids {
        let adv = face.glyph_hor_advance(ttf_parser::GlyphId(g)).unwrap_or(0);
        advance_by_gid.insert(g, (f32::from(adv) * scale).round().clamp(0.0, f32::from(u16::MAX)) as u16);
    }

    // /FontFile2 (raw subset bytes; /Length1 is the uncompressed length).
    let len1 = i64::try_from(subset.len()).unwrap_or(i64::MAX);
    let mut ff_dict = Dictionary::new();
    ff_dict.set("Length1", Object::Integer(len1));
    let font_file = doc.add_object(Stream::new(ff_dict, subset));

    let base_name = format!("{}+VibeEmbedded", subset_tag());

    // /FontDescriptor.
    let bbox = face.global_bounding_box();
    let flags = 4 | (i64::from(u8::from(face.is_italic())) * 64); // Symbolic (+ Italic)
    let cap = face.capital_height().map_or_else(|| i32::from(face.ascender()), i32::from);
    let mut fd = Dictionary::new();
    fd.set("Type", Object::Name(b"FontDescriptor".to_vec()));
    fd.set("FontName", Object::Name(base_name.clone().into_bytes()));
    fd.set("Flags", Object::Integer(flags));
    fd.set(
        "FontBBox",
        Object::Array(vec![
            Object::Integer(scaled(i32::from(bbox.x_min))),
            Object::Integer(scaled(i32::from(bbox.y_min))),
            Object::Integer(scaled(i32::from(bbox.x_max))),
            Object::Integer(scaled(i32::from(bbox.y_max))),
        ]),
    );
    fd.set("ItalicAngle", Object::Real(face.italic_angle()));
    fd.set("Ascent", Object::Integer(scaled(i32::from(face.ascender()))));
    fd.set("Descent", Object::Integer(scaled(i32::from(face.descender()))));
    fd.set("CapHeight", Object::Integer(scaled(cap)));
    fd.set("StemV", Object::Integer(80));
    fd.set("FontFile2", Object::Reference(font_file));
    let fd_id = doc.add_object(Object::Dictionary(fd));

    // /W widths: `[gid [w] gid [w] …]`.
    let mut w = Vec::with_capacity(advance_by_gid.len() * 2);
    for (&g, &adv) in &advance_by_gid {
        w.push(Object::Integer(i64::from(g)));
        w.push(Object::Array(vec![Object::Integer(i64::from(adv))]));
    }

    // Descendant CIDFontType2.
    let mut csi = Dictionary::new();
    csi.set("Registry", Object::string_literal("Adobe"));
    csi.set("Ordering", Object::string_literal("Identity"));
    csi.set("Supplement", Object::Integer(0));
    let mut cid = Dictionary::new();
    cid.set("Type", Object::Name(b"Font".to_vec()));
    cid.set("Subtype", Object::Name(b"CIDFontType2".to_vec()));
    cid.set("BaseFont", Object::Name(base_name.clone().into_bytes()));
    cid.set("CIDSystemInfo", Object::Dictionary(csi));
    cid.set("FontDescriptor", Object::Reference(fd_id));
    cid.set("CIDToGIDMap", Object::Name(b"Identity".to_vec()));
    cid.set("DW", Object::Integer(1000));
    cid.set("W", Object::Array(w));
    let cid_id = doc.add_object(Object::Dictionary(cid));

    // /ToUnicode CMap (so the text stays copyable/searchable).
    let unicode_cmap = build_tounicode(&gid_by_char);
    let to_unicode = doc.add_object(Stream::new(Dictionary::new(), unicode_cmap.into_bytes()));

    // Type0 (Identity-H).
    let mut type0 = Dictionary::new();
    type0.set("Type", Object::Name(b"Font".to_vec()));
    type0.set("Subtype", Object::Name(b"Type0".to_vec()));
    type0.set("BaseFont", Object::Name(base_name.into_bytes()));
    type0.set("Encoding", Object::Name(b"Identity-H".to_vec()));
    type0.set("DescendantFonts", Object::Array(vec![Object::Reference(cid_id)]));
    type0.set("ToUnicode", Object::Reference(to_unicode));

    Ok(CidFont { font_dict: type0, gid_by_char, advance_by_gid })
}

/// One embedded-Unicode run to place in a **page** content stream: the CID text
/// plus its style + placement. The page-content analogue of the retired
/// `font_embed::EmbedRun` — the CID-path unification routes the page-content
/// writers (header/footer, watermark, text box) through the same hand-built CID
/// font the `/AP` writers use, so they gain exact metrics *and* the HF2
/// marked-content tag, and lose the `PDFium` round-trip.
pub(crate) struct CidRun<'a> {
    pub text: &'a str,
    pub size: f32,
    pub color: (f32, f32, f32),
    /// Text-space → page-space matrix (position + rotation), applied as a `cm`.
    pub matrix: [f32; 6],
    /// Fill opacity `0.0..=1.0`, forwarded via an `ExtGState` `/ca`+`/CA`.
    /// `1.0` emits no graphics-state change.
    pub opacity: f32,
    /// Draw *under* existing content (prepend) rather than on top (append) —
    /// a "behind" watermark.
    pub behind: bool,
    /// When `Some(width)`, draw an underline rule `width` points long beneath the
    /// text (rides the same `matrix`). `None` leaves it un-ruled.
    pub underline: Option<f32>,
    /// Marked-content `/Kind`, so the fragment is splice-removable (HF2).
    pub kind: &'a str,
}

/// Place one [`CidRun`] into `page_id`'s content stream, wrapped in the HF2
/// `/VibePDF … BDC … EMC` marked-content tag. `font_name` is a `/Font` resource
/// (registered once per page by the caller) that references `cid`'s dict. Any
/// `ExtGState` needed for opacity is registered here. lopdf only — no `PDFium`.
pub(crate) fn place_cid_run(
    doc: &mut Document,
    page_id: ObjectId,
    font_name: &str,
    cid: &CidFont,
    run: &CidRun,
) -> Result<(), CommandError> {
    use crate::pdf::cos::{
        append_page_content, prepend_page_content, register_page_resource, visual_cm_line,
        wrap_decoration,
    };

    let (r, g, b) = run.color;
    let mut inner = String::new();
    let _ = writeln!(inner, "q");
    if run.opacity < 1.0 {
        let mut gs = Dictionary::new();
        gs.set("Type", Object::Name(b"ExtGState".to_vec()));
        gs.set("ca", Object::Real(run.opacity));
        gs.set("CA", Object::Real(run.opacity));
        let gs_name =
            register_page_resource(doc, page_id, b"ExtGState", "GSvibe", Object::Dictionary(gs))?;
        let _ = writeln!(inner, "/{gs_name} gs");
    }
    let _ = writeln!(inner, "{}", visual_cm_line(run.matrix));
    let _ = writeln!(inner, "{r:.4} {g:.4} {b:.4} rg");
    let _ = writeln!(
        inner,
        "BT\n/{font_name} {size:.2} Tf\n0 0 Td\n<{hex}> Tj\nET",
        size = run.size,
        hex = cid.encode_hex(run.text),
    );
    if let Some(width) = run.underline {
        // A thin rule just below the baseline, riding the same matrix.
        let y = -0.12 * run.size;
        let thick = (run.size * 0.06).max(0.5);
        let _ = writeln!(
            inner,
            "{r:.4} {g:.4} {b:.4} RG\n{thick:.2} w\n0 {y:.2} m\n{width:.2} {y:.2} l\nS",
        );
    }
    let _ = writeln!(inner, "Q");

    let content = wrap_decoration(run.kind, inner);
    if run.behind {
        prepend_page_content(doc, page_id, content)
    } else {
        append_page_content(doc, page_id, content)
    }
}

/// A 6-uppercase-letter subset tag (PDF convention: `ABCDEF+FontName`), unique
/// per call so two embedded subsets in one document never collide under a name.
fn subset_tag() -> String {
    uuid::Uuid::new_v4()
        .as_bytes()
        .iter()
        .take(6)
        .map(|b| char::from(b'A' + b % 26))
        .collect()
}

/// A `/ToUnicode` `CMap` mapping each glyph id back to its Unicode scalar, in
/// `beginbfchar` blocks of ≤100 (the format's limit).
fn build_tounicode(gid_by_char: &BTreeMap<char, u16>) -> String {
    let mut entries: Vec<(u16, char)> = gid_by_char.iter().map(|(&c, &g)| (g, c)).collect();
    entries.sort_unstable();

    let mut body = String::new();
    for chunk in entries.chunks(100) {
        let _ = writeln!(body, "{} beginbfchar", chunk.len());
        for (gid, ch) in chunk {
            let _ = writeln!(body, "<{gid:04X}> <{}>", utf16be_hex(*ch));
        }
        body.push_str("endbfchar\n");
    }
    format!(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
         1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n\
         {body}endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend"
    )
}

/// UTF-16BE hex of a scalar (4 hex for BMP, 8 for a surrogate pair).
fn utf16be_hex(ch: char) -> String {
    let mut buf = [0u16; 2];
    let mut out = String::new();
    for unit in ch.encode_utf16(&mut buf).iter() {
        let _ = write!(out, "{unit:04X}");
    }
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use pdfium_render::prelude::*;

    use super::*;
    use crate::pdf::cos::{append_page_content, register_page_resource};
    use crate::pdf::document::{pdfium, pdfium_lock};

    fn hello() -> Vec<u8> {
        std::fs::read("../tests/fixtures/basic/hello.pdf").expect("hello.pdf")
    }

    fn coptic_font() -> Vec<u8> {
        std::fs::read("../tests/fixtures/fonts/NotoSansCoptic-Regular.ttf").expect("coptic font")
    }

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

    /// P4.HF9 make-or-break spike: a hand-built CID font, placed in page content,
    /// must **render + re-extract** its Unicode through `PDFium`. If this fails, the
    /// hand-built-CID approach is wrong — stop and report before wiring `/AP`.
    #[test]
    fn pdfium_renders_and_extracts_hand_built_cid_font() {
        let text = "\u{2C81}\u{2C83}\u{2C85}"; // Ⲁ Ⲃ Ⲅ
        let mut doc = Document::load_mem(&hello()).expect("load");
        let page_id = *doc.get_pages().get(&1).expect("page 1");
        let cid = build_cid_font(&mut doc, &coptic_font(), text).expect("build cid font");
        let font_name = register_page_resource(
            &mut doc,
            page_id,
            b"Font",
            "Fcid",
            Object::Dictionary(cid.font_dict.clone()),
        )
        .expect("register font");
        let content = format!("q\nBT\n/{font_name} 24 Tf\n72 700 Td\n<{}> Tj\nET\nQ\n", cid.encode_hex(text));
        append_page_content(&mut doc, page_id, content).expect("append");
        let mut out = Vec::new();
        doc.save_to(&mut out).expect("save");

        assert!(page0_text(&out).contains(text), "hand-built CID text must render + extract");
    }

    /// The dict graph is shaped like a valid embedded CID font.
    #[test]
    fn builds_type0_cidfont_with_tounicode() {
        let mut doc = Document::with_version("1.5");
        let cid = build_cid_font(&mut doc, &coptic_font(), "\u{2C81}\u{2C83}").expect("build");
        assert_eq!(
            cid.font_dict.get(b"Subtype").unwrap().as_name().unwrap(),
            b"Type0",
        );
        assert_eq!(
            cid.font_dict.get(b"Encoding").unwrap().as_name().unwrap(),
            b"Identity-H",
        );
        assert!(cid.font_dict.get(b"ToUnicode").is_ok(), "has /ToUnicode");
        // Serialize and confirm the descendant + font program landed in `doc`.
        // (The Type0 itself is returned to inline in an /AP, not added to `doc`;
        // its /Identity-H encoding is asserted directly on `font_dict` above.)
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        for needle in [&b"/CIDFontType2"[..], b"/FontFile2", b"/CIDToGIDMap"] {
            assert!(
                bytes.windows(needle.len()).any(|w| w == needle),
                "missing {:?}",
                std::str::from_utf8(needle).unwrap(),
            );
        }
    }

    /// Encoding is 4 hex per char; an uncovered char maps to `0000`.
    #[test]
    fn encodes_identity_h_gids() {
        let mut doc = Document::with_version("1.5");
        let cid = build_cid_font(&mut doc, &coptic_font(), "\u{2C81}").expect("build");
        let hex = cid.encode_hex("\u{2C81}z"); // Coptic (covered) + 'z' (not in this face)
        assert_eq!(hex.len(), 8, "two chars → 8 hex");
        assert!(hex.ends_with("0000"), "uncovered 'z' → .notdef; got {hex}");
        assert!(!hex.starts_with("0000"), "covered Coptic → a real gid; got {hex}");
    }
}
