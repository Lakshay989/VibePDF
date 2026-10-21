//! Font subsetting in Compress (P7.C2c, SPEC P7-OCR-010).
//!
//! The property that matters is that nothing changes on the page. A subset
//! that drops a glyph the document draws makes that text vanish with no error
//! anywhere, so every test here renders each page before and after, with
//! PDFium, and requires them to be identical, besides checking the size.
//!
//! `whole-font.pdf` embeds the whole of the committed Noto Sans Coptic twice, as
//! some report generators embed whole fonts: once in a composite font, once in
//! a simple TrueType font. It is built by the ignored test at the bottom.

use std::path::PathBuf;

use lopdf::{dictionary, Document, Object, Stream};
use vibepdf_lib::pdf::compress::{compress_bytes, CompressLevel};
use vibepdf_lib::pdf::document::open_pdf;
use vibepdf_lib::pdf::render::{render_page, ImageFormat};

fn fixture(name: &str) -> Vec<u8> {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    std::fs::read(p).expect("read fixture")
}

/// Every page as PDFium renders it, RGBA at 72 DPI.
fn renders(bytes: &[u8]) -> Vec<Vec<u8>> {
    let path = std::env::temp_dir().join(format!("vibepdf-fonts-{}.pdf", uuid::Uuid::new_v4()));
    std::fs::write(&path, bytes).expect("write temp pdf");
    let pages = {
        let (doc, meta) = open_pdf(&path, None).expect("PDFium opens it");
        (0..meta.page_count)
            .map(|page| render_page(&doc, page, 72.0, ImageFormat::Rgba8).expect("render").bytes)
            .collect()
    };
    let _ = std::fs::remove_file(&path);
    pages
}

/// Every page's text, as the text layer extracts it.
fn texts(bytes: &[u8]) -> Vec<String> {
    let path = std::env::temp_dir().join(format!("vibepdf-fonts-{}.pdf", uuid::Uuid::new_v4()));
    std::fs::write(&path, bytes).expect("write temp pdf");
    let pages = {
        let (doc, meta) = open_pdf(&path, None).expect("PDFium opens it");
        (0..meta.page_count)
            .map(|page| {
                let runs = vibepdf_lib::pdf::text_extract::extract_text_runs(&doc, page as usize).expect("extract");
                runs.iter().map(|r| r.text.as_str()).collect()
            })
            .collect()
    };
    let _ = std::fs::remove_file(&path);
    pages
}

/// The stored size of each of the document's `FontFile2`s, smallest first.
fn font_file_lens(bytes: &[u8]) -> Vec<usize> {
    let doc = Document::load_mem(bytes).expect("load");
    let mut lens: Vec<usize> = doc
        .objects
        .values()
        .filter_map(|o| o.as_dict().ok())
        .filter_map(|d| d.get(b"FontFile2").and_then(Object::as_reference).ok())
        .map(|file| doc.get_object(file).and_then(Object::as_stream).expect("stream").content.len())
        .collect();
    lens.sort_unstable();
    lens
}

// SPEC: P7-OCR-010 — "targeting ... font subsetting". Both whole fonts are cut
// down to the glyphs their pages draw, and nothing on any page moves.
#[test]
fn subsetting_whole_fonts_keeps_every_page_identical() {
    let before = fixture("whole-font.pdf");
    let (after, report) = compress_bytes(&before, CompressLevel::Medium).expect("compress");

    assert_eq!((report.fonts_subset, report.fonts_kept), (2, 0), "{report:?}");
    let (whole, subset) = (font_file_lens(&before), font_file_lens(&after));
    assert_eq!(subset.len(), 2);
    for (whole, subset) in whole.iter().zip(&subset) {
        assert!(subset * 2 < *whole, "a font went from {whole} to only {subset} bytes");
    }
    assert!(after.len() < before.len());

    let pages = renders(&before);
    // Identical renders prove nothing if a page draws no glyphs at all, as it
    // would if a font's codes reached no glyph even before subsetting.
    for (i, page) in pages.iter().enumerate() {
        let ink = page.chunks(4).filter(|px| px[0] < 128).count();
        assert!(ink > 500, "page {} draws almost nothing ({ink} dark pixels)", i + 1);
    }
    assert_eq!(pages, renders(&after), "a page renders differently after subsetting");
    let text = texts(&after);
    assert_eq!(texts(&before), text, "the text layer changed");
    assert!(text[0].contains("ⲁⲃⲅⲇ") && text[1].contains("ⲉⲍⲏⲑ") && text[2].contains("ⲕⲗⲙⲛ"), "{text:?}");
}

// Low promises nothing lost, so its fonts keep every glyph.
#[test]
fn low_keeps_every_font_whole() {
    let before = fixture("whole-font.pdf");
    let (after, report) = compress_bytes(&before, CompressLevel::Low).expect("compress");
    assert_eq!((report.fonts_subset, report.fonts_kept), (0, 2), "{report:?}");
    let doc = Document::load_mem(&after).expect("load");
    let whole = std::fs::read("../tests/fixtures/fonts/NotoSansCoptic-Regular.ttf").unwrap();
    for file in doc.objects.values().filter_map(|o| o.as_dict().ok()).filter_map(|d| d.get(b"FontFile2").and_then(Object::as_reference).ok()) {
        let program = doc.get_object(file).and_then(Object::as_stream).unwrap().decompressed_content().unwrap();
        assert_eq!(program, whole);
    }
}

// A font that is already a subset (VibePDF's own) has nothing to give, and
// whatever happens to it, the page must look the same.
#[test]
fn an_existing_subset_still_renders_the_same() {
    let before = fixture("unicode-text.pdf");
    let (after, _) = compress_bytes(&before, CompressLevel::High).expect("compress");
    assert_eq!(renders(&before), renders(&after));
    assert_eq!(texts(&before), texts(&after));
}

/// Writes `tests/fixtures/basic/whole-font.pdf`: three pages of Coptic, each
/// drawing four different letters. Pages 1 and 2 use a composite font
/// (`Identity-H`); page 3 a simple TrueType font whose `/Differences` name the
/// font's own glyphs. Each font embeds its own copy of the *whole* Noto Sans
/// Coptic. `ToUnicode` maps make the text selectable, and the streams are
/// deflated, as a producer would write them.
#[test]
#[ignore = "regenerates a committed fixture; run on demand"]
fn writes_the_whole_font_fixture() {
    let font = std::fs::read("../tests/fixtures/fonts/NotoSansCoptic-Regular.ttf").expect("font");
    let face = ttf_parser::Face::parse(&font, 0).expect("face");
    let scale = 1000.0 / f32::from(face.units_per_em());
    let lines = ["ⲁⲃⲅⲇ", "ⲉⲍⲏⲑ"];
    let gid = |c: char| face.glyph_index(c).expect("a Coptic glyph").0;

    let mut doc = Document::with_version("1.7");
    let file = doc.add_object(Stream::new(dictionary! { "Length1" => font.len() as i64 }, font.clone()));
    let descriptor = doc.add_object(dictionary! {
        "Type" => "FontDescriptor", "FontName" => "NotoSansCoptic-Regular", "Flags" => 4,
        "FontBBox" => vec![(-500).into(), (-300).into(), 1500.into(), 1000.into()], "ItalicAngle" => 0,
        "Ascent" => 1000, "Descent" => -300, "CapHeight" => 700, "StemV" => 80, "FontFile2" => file,
    });
    let mut widths = Vec::new();
    let mut cmap = String::from(
        "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n/CMapName /Coptic-UCS def\n\
         1 begincodespacerange <0000> <FFFF> endcodespacerange\n",
    );
    let chars: Vec<char> = lines.iter().flat_map(|l| l.chars()).collect();
    cmap.push_str(&format!("{} beginbfchar\n", chars.len()));
    for c in &chars {
        let g = gid(*c);
        let advance = face.glyph_hor_advance(ttf_parser::GlyphId(g)).unwrap_or(0);
        widths.push(Object::Integer(i64::from(g)));
        widths.push(Object::Array(vec![Object::Integer((f32::from(advance) * scale).round() as i64)]));
        cmap.push_str(&format!("<{:04X}> <{:04X}>\n", g, *c as u32));
    }
    cmap.push_str("endbfchar\nendcmap CMapName currentdict /CMap defineresource pop end end\n");
    let to_unicode = doc.add_object(Stream::new(Dictionary::new(), cmap.into_bytes()));
    let cid = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "CIDFontType2", "BaseFont" => "NotoSansCoptic-Regular",
        "CIDSystemInfo" => dictionary! {
            "Registry" => Object::string_literal("Adobe"),
            "Ordering" => Object::string_literal("Identity"), "Supplement" => 0,
        },
        "FontDescriptor" => descriptor, "CIDToGIDMap" => "Identity", "W" => widths,
    });
    let type0 = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type0", "BaseFont" => "NotoSansCoptic-Regular",
        "Encoding" => "Identity-H", "DescendantFonts" => vec![cid.into()], "ToUnicode" => to_unicode,
    });

    // Page 3's simple TrueType font: codes a, b, c, d name the glyphs of ⲕⲗⲙⲛ.
    let simple_line = "ⲕⲗⲙⲛ";
    let simple_file = doc.add_object(Stream::new(dictionary! { "Length1" => font.len() as i64 }, font.clone()));
    let simple_descriptor = doc.add_object(dictionary! {
        "Type" => "FontDescriptor", "FontName" => "NotoSansCoptic-Regular", "Flags" => 32,
        "FontBBox" => vec![(-500).into(), (-300).into(), 1500.into(), 1000.into()], "ItalicAngle" => 0,
        "Ascent" => 1000, "Descent" => -300, "CapHeight" => 700, "StemV" => 80, "FontFile2" => simple_file,
    });
    let mut differences = vec![Object::Integer(97)];
    let mut simple_widths = Vec::new();
    let mut simple_cmap = String::from(
        "/CIDInit /ProcSet findresource begin 12 dict begin begincmap\n/CMapName /Coptic-Simple def\n\
         1 begincodespacerange <00> <FF> endcodespacerange\n4 beginbfchar\n",
    );
    for (i, c) in simple_line.chars().enumerate() {
        let g = ttf_parser::GlyphId(gid(c));
        differences.push(Object::Name(face.glyph_name(g).expect("a post name").as_bytes().to_vec()));
        let advance = face.glyph_hor_advance(g).unwrap_or(0);
        simple_widths.push(Object::Integer((f32::from(advance) * scale).round() as i64));
        simple_cmap.push_str(&format!("<{:02X}> <{:04X}>\n", 97 + i, c as u32));
    }
    simple_cmap.push_str("endbfchar\nendcmap CMapName currentdict /CMap defineresource pop end end\n");
    let simple_to_unicode = doc.add_object(Stream::new(Dictionary::new(), simple_cmap.into_bytes()));
    let simple_font = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "TrueType", "BaseFont" => "NotoSansCoptic-Regular",
        "FontDescriptor" => simple_descriptor, "FirstChar" => 97, "LastChar" => 100, "Widths" => simple_widths,
        "Encoding" => dictionary! { "Type" => "Encoding", "BaseEncoding" => "WinAnsiEncoding", "Differences" => differences },
        "ToUnicode" => simple_to_unicode,
    });

    let pages = doc.new_object_id();
    let mut kids = Vec::new();
    for line in lines {
        let hex: String = line.chars().map(|c| format!("{:04X}", gid(c))).collect();
        let content = format!("BT /F1 36 Tf 72 700 Td <{hex}> Tj ET");
        let contents = doc.add_object(Stream::new(Dictionary::new(), content.into_bytes()));
        kids.push(Object::Reference(doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages, "Contents" => contents,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => type0 } },
        })));
    }
    let contents = doc.add_object(Stream::new(Dictionary::new(), b"BT /F2 36 Tf 72 700 Td (abcd) Tj ET".to_vec()));
    kids.push(Object::Reference(doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages, "Contents" => contents,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Resources" => dictionary! { "Font" => dictionary! { "F2" => simple_font } },
    })));
    doc.objects.insert(pages, Object::Dictionary(dictionary! { "Type" => "Pages", "Count" => kids.len() as i64, "Kids" => kids }));
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);
    doc.compress();
    doc.save("../tests/fixtures/basic/whole-font.pdf").expect("write fixture");
}

use lopdf::Dictionary;
