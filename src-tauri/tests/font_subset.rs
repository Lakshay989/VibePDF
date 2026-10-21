//! Font subsetting in Compress (P7.C2c, SPEC P7-OCR-010).
//!
//! The property that matters is that nothing changes on the page. A subset
//! that drops a glyph the document draws makes that text vanish with no error
//! anywhere, so every test here renders each page before and after, with
//! PDFium, and requires them to be identical, besides checking the size.
//!
//! `whole-font.pdf` embeds the whole of the committed Noto Sans Coptic, as some
//! report generators embed whole fonts; it is built by the ignored test at the
//! bottom.

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

/// The stored size of the document's one `FontFile2`.
fn font_file_len(bytes: &[u8]) -> usize {
    let doc = Document::load_mem(bytes).expect("load");
    let file = doc
        .objects
        .values()
        .filter_map(|o| o.as_dict().ok())
        .find_map(|d| d.get(b"FontFile2").and_then(Object::as_reference).ok())
        .expect("a FontFile2");
    doc.get_object(file).and_then(Object::as_stream).expect("stream").content.len()
}

// SPEC: P7-OCR-010 — "targeting ... font subsetting". The whole font is cut
// down to the eight glyphs the two pages draw, and nothing on either page moves.
#[test]
fn subsetting_a_whole_font_keeps_every_page_identical() {
    let before = fixture("whole-font.pdf");
    let (after, report) = compress_bytes(&before, CompressLevel::Medium).expect("compress");

    assert_eq!((report.fonts_subset, report.fonts_kept), (1, 0), "{report:?}");
    let (whole, subset) = (font_file_len(&before), font_file_len(&after));
    assert!(subset * 2 < whole, "the font went from {whole} to only {subset} bytes");
    assert!(after.len() < before.len());

    assert_eq!(renders(&before), renders(&after), "a page renders differently after subsetting");
    let text = texts(&after);
    assert_eq!(texts(&before), text, "the text layer changed");
    assert!(text[0].contains("ⲁⲃⲅⲇ") && text[1].contains("ⲉⲍⲏⲑ"), "{text:?}");
}

// Low promises nothing lost, so its fonts keep every glyph.
#[test]
fn low_keeps_every_font_whole() {
    let before = fixture("whole-font.pdf");
    let (after, report) = compress_bytes(&before, CompressLevel::Low).expect("compress");
    assert_eq!((report.fonts_subset, report.fonts_kept), (0, 1), "{report:?}");
    let doc = Document::load_mem(&after).expect("load");
    let file = doc
        .objects
        .values()
        .filter_map(|o| o.as_dict().ok())
        .find_map(|d| d.get(b"FontFile2").and_then(Object::as_reference).ok())
        .expect("a FontFile2");
    let program = doc.get_object(file).and_then(Object::as_stream).unwrap().decompressed_content().unwrap();
    assert_eq!(program, std::fs::read("../tests/fixtures/fonts/NotoSansCoptic-Regular.ttf").unwrap());
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

/// Writes `tests/fixtures/basic/whole-font.pdf`: two pages of Coptic, each
/// drawing four different letters, in a composite font (`Identity-H`) that
/// embeds the *whole* Noto Sans Coptic. A `ToUnicode` map makes the text
/// selectable, and the streams are deflated, as a producer would write them.
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
    doc.objects.insert(pages, Object::Dictionary(dictionary! { "Type" => "Pages", "Count" => kids.len() as i64, "Kids" => kids }));
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);
    doc.compress();
    doc.save("../tests/fixtures/basic/whole-font.pdf").expect("write fixture");
}

use lopdf::Dictionary;
