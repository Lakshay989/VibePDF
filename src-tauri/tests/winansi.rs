//! WinAnsi text correctness (FABLE_REVIEW 3.2).
//!
//! The built-in base-14 fonts can only render the `WinAnsiEncoding` (CP1252)
//! range. Text writers now (a) build fonts with `/Encoding /WinAnsiEncoding`,
//! (b) transcode Latin-1 / CP1252 characters to octal escapes of their WinAnsi
//! byte, and (c) reject anything outside that range with a typed error — instead
//! of silently emitting mojibake. These tests exercise all three through the
//! real writers.

use lopdf::{Document, Object};
use vibepdf_lib::error::CommandError;
use vibepdf_lib::pdf::cos::{add_free_text, add_text_box, update_free_text};
use vibepdf_lib::pdf::header_footer::add_header_footer;
use vibepdf_lib::pdf::watermark::{add_watermark, WatermarkKind};

fn hello() -> Vec<u8> {
    std::fs::read("../tests/fixtures/basic/hello.pdf").expect("hello.pdf")
}

fn text_kind(text: &str) -> WatermarkKind {
    WatermarkKind::Text {
        text: text.to_owned(),
        font_family: "Helvetica".to_owned(),
        size: 40.0,
        color: "#000000".to_owned(),
        bold: false,
        italic: false,
    }
}

/// The decoded page-1 content of `bytes`.
fn page1(bytes: &[u8]) -> String {
    let doc = Document::load_mem(bytes).expect("load");
    let id = *doc.get_pages().get(&1).expect("page 1");
    String::from_utf8_lossy(&doc.get_page_content(id).expect("content")).into_owned()
}

/// Does the document declare a `/WinAnsiEncoding` font? Font dictionaries are
/// written uncompressed by lopdf, so a byte search is exact and shape-agnostic
/// (the font sits deep inside the page `/Resources /Font`).
fn has_winansi_font(bytes: &[u8]) -> bool {
    bytes.windows(b"WinAnsiEncoding".len()).any(|w| w == b"WinAnsiEncoding")
}

#[test]
fn latin1_watermark_transcodes_to_winansi_octal() {
    // "Café" → the é (U+00E9) becomes octal \351 (0o351 == 0xE9).
    let out = add_watermark(&hello(), &[0], &text_kind("Café"), 0.3, 0.0, true).expect("wm");
    let c = page1(&out);
    assert!(c.contains(r"Caf\351) Tj"), "é transcoded to \\351; got:\n{c}");
    assert!(has_winansi_font(&out), "the font declares /WinAnsiEncoding");
}

#[test]
fn cp1252_punctuation_transcodes() {
    // Smart apostrophe (U+2019 → 0x92), en dash (U+2013 → 0x96), euro (U+20AC → 0x80).
    let out = add_watermark(&hello(), &[0], &text_kind("it\u{2019}s \u{2013} \u{20AC}5"), 0.3, 0.0, true)
        .expect("wm");
    let c = page1(&out);
    assert!(c.contains(r"it\222s \226 \2005) Tj"), "CP1252 punctuation → octal; got:\n{c}");
}

#[test]
fn ascii_output_is_byte_stable() {
    // Plain ASCII must pass through unescaped (regression guard).
    let out = add_watermark(&hello(), &[0], &text_kind("DRAFT 50%"), 0.3, 0.0, true).expect("wm");
    assert!(page1(&out).contains("(DRAFT 50%) Tj"), "ASCII unchanged");
}

#[test]
fn parens_and_backslash_still_escaped() {
    let out = add_watermark(&hello(), &[0], &text_kind(r"a(b)c\d"), 0.3, 0.0, true).expect("wm");
    assert!(page1(&out).contains(r"(a\(b\)c\\d) Tj"), "delimiters escaped");
}

fn is_invalid(r: Result<Vec<u8>, CommandError>) -> bool {
    matches!(r, Err(CommandError::InvalidInput(_)))
}

#[test]
fn non_winansi_watermark_rejected() {
    assert!(is_invalid(add_watermark(&hello(), &[0], &text_kind("日本語"), 0.3, 0.0, true)));
}

#[test]
fn non_winansi_header_footer_rejected() {
    let r = add_header_footer(
        &hello(), &[0], "footer", "", "日付", "", "Helvetica", 10.0, "#000000", 36.0, "d",
    );
    assert!(is_invalid(r), "CJK in a header position must be rejected");
    // The date field is validated too.
    let r2 = add_header_footer(
        &hello(), &[0], "footer", "ok", "", "", "Helvetica", 10.0, "#000000", 36.0, "２０２６",
    );
    assert!(is_invalid(r2), "fullwidth digits in {{date}} must be rejected");
}

#[test]
fn non_winansi_text_box_rejected() {
    let r = add_text_box(&hello(), 0, [72.0, 600.0, 300.0, 640.0], "日本語", "Helvetica", 12.0, "#000000", false, false, false);
    assert!(is_invalid(r));
}

#[test]
fn non_winansi_free_text_rejected() {
    let add = add_free_text(&hello(), 0, [72.0, 600.0, 300.0, 640.0], "日本語", "Helvetica", 12.0, "#000000", false, false, false);
    assert!(is_invalid(add), "add");
    // update rejects too (a re-edit into non-WinAnsi text).
    let ok = add_free_text(&hello(), 0, [72.0, 600.0, 300.0, 640.0], "ok", "Helvetica", 12.0, "#000000", false, false, false)
        .expect("seed free text");
    let doc = Document::load_mem(&ok).expect("load");
    let mut nm = None;
    for o in doc.objects.values() {
        let Ok(d) = o.as_dict() else { continue };
        if d.get(b"Subtype").and_then(Object::as_name).ok() == Some(&b"FreeText"[..]) {
            if let Ok(v) = d.get(b"NM").and_then(Object::as_str) {
                nm = Some(String::from_utf8_lossy(v).into_owned());
                break;
            }
        }
    }
    let nm = nm.expect("a /NM on the free-text annotation");
    let upd = update_free_text(&ok, &nm, "日本語", "Helvetica", 12.0, "#000000", false, false, false);
    assert!(is_invalid(upd), "update");
}

#[test]
fn error_names_the_offending_characters() {
    // Four distinct offenders; the list is capped at three.
    match add_watermark(&hello(), &[0], &text_kind("a日b本c語d文e"), 0.3, 0.0, true) {
        Err(CommandError::InvalidInput(m)) => {
            assert!(m.contains('日') && m.contains('本') && m.contains('語'), "names offenders: {m}");
            assert!(!m.contains('文'), "caps the list at three: {m}");
        }
        other => panic!("expected InvalidInput, got {other:?}"),
    }
}

/// Writes a Latin-1 / CP1252 text artifact to the git-ignored `Sample PDFs/`
/// for the cross-reader ritual: accents, smart punctuation, en/em dashes, and €
/// must all render correctly (previously mojibake). Ignored; run on demand:
///   cargo test --test winansi winansi_writes_verification_artifact -- --ignored
#[test]
#[ignore = "produces a verification artifact; run on demand"]
fn winansi_writes_verification_artifact() {
    let out = std::path::PathBuf::from("../Sample PDFs/vibepdf-verify-winansi.pdf");
    if let Some(par) = out.parent() {
        std::fs::create_dir_all(par).ok();
    }
    let a = add_watermark(&hello(), &[0], &text_kind("Café résumé"), 0.25, 45.0, true)
        .expect("watermark");
    let b = add_header_footer(
        &a, &[0], "footer", "", "Página 1 \u{2013} 50 %", "", "Helvetica", 10.0, "#333333", 36.0, "d",
    )
    .expect("footer");
    let c = add_free_text(
        &b, 0, [72.0, 620.0, 460.0, 660.0], "\u{201C}na\u{00EF}ve\u{201D} \u{2014} \u{20AC}5",
        "Helvetica", 16.0, "#000000", false, false, false,
    )
    .expect("free text");
    std::fs::write(&out, &c).expect("write artifact");
    eprintln!("wrote {}", out.display());
}
