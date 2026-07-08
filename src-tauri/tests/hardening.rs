//! P4.HF hardening tests (FABLE_REVIEW items 3.7 and 3.3).
//!
//! - 3.7: `/Contents` may legally be an **indirect reference to an array**;
//!   appending page content must preserve every original stream (not nest the
//!   array inside the new `/Contents` array).
//! - 3.3: pin what actually happens when an encrypted document is opened with
//!   its password and then saved.

use std::path::PathBuf;

use lopdf::{dictionary, Dictionary, Document, Object, Stream};
use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::cos::add_text_box;
use vibepdf_lib::pdf::document::open_pdf;

/// Build a minimal one-page PDF whose `/Contents` is a **Reference → Array** of
/// two separate streams — the shape FABLE_REVIEW 3.7 flagged as mishandled.
fn doc_with_indirect_contents_array() -> Vec<u8> {
    let mut doc = Document::with_version("1.4");
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let s1 = doc.add_object(Stream::new(
        Dictionary::new(),
        b"BT /F1 12 Tf 72 700 Td (first) Tj ET".to_vec(),
    ));
    let s2 = doc.add_object(Stream::new(
        Dictionary::new(),
        b"BT /F1 12 Tf 72 680 Td (second) Tj ET".to_vec(),
    ));
    // The trap: /Contents is an indirect reference to this array.
    let contents_arr = doc.add_object(Object::Array(vec![
        Object::Reference(s1),
        Object::Reference(s2),
    ]));

    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => Object::Reference(pages_id),
        "MediaBox" => Object::Array(vec![0.into(), 0.into(), 612.into(), 792.into()]),
        "Resources" => dictionary! { "Font" => dictionary! { "F1" => Object::Reference(font_id) } },
        "Contents" => Object::Reference(contents_arr),
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => Object::Array(vec![Object::Reference(page_id)]),
            "Count" => 1,
        }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => Object::Reference(pages_id) });
    doc.trailer.set("Root", Object::Reference(catalog));
    let mut buf = Vec::new();
    doc.save_to(&mut buf).expect("save synthetic doc");
    buf
}

/// SPEC: P4-EDIT-003 (the appending writer) — appending to a `/Contents` that is
/// an indirect reference to an array must keep every original stream *and*
/// produce a flat, all-stream `/Contents` array (no nested array element).
#[test]
fn contents_indirect_array_shape_preserved() {
    let src = doc_with_indirect_contents_array();
    let out = add_text_box(
        &src,
        0,
        [72.0, 600.0, 300.0, 640.0],
        "added",
        "Helvetica",
        12.0,
        "#000000",
        false,
        false,
        false,
    )
    .expect("append to ref→array /Contents");

    let doc = Document::load_mem(&out).expect("reload");
    let page_id = *doc.get_pages().get(&1).expect("page 1");

    // Every /Contents element must resolve to a Stream — a nested Array element
    // is the corruption this guards against.
    let contents = doc
        .get_dictionary(page_id)
        .unwrap()
        .get(b"Contents")
        .expect("/Contents");
    let elements: Vec<Object> = match contents {
        Object::Array(a) => a.clone(),
        Object::Reference(id) => doc.get_object(*id).unwrap().as_array().unwrap().clone(),
        other => panic!("unexpected /Contents shape: {other:?}"),
    };
    assert_eq!(elements.len(), 3, "two originals + the appended stream");
    for el in &elements {
        let id = el.as_reference().expect("element is a reference");
        assert!(
            doc.get_object(id).unwrap().as_stream().is_ok(),
            "every /Contents element resolves to a stream (no nested array)"
        );
    }

    // Both original streams still paint, plus the new one.
    let all = String::from_utf8_lossy(&doc.get_page_content(page_id).unwrap()).into_owned();
    for needle in ["(first) Tj", "(second) Tj", "added"] {
        assert!(all.contains(needle), "content keeps {needle}; got:\n{all}");
    }
}

/// Writes a rotated-pages PDF carrying a footer + watermark to the git-ignored
/// `Sample PDFs/` for the manual cross-reader ritual: on every page (including
/// /Rotate 90/180/270) the footer must read upright at the visual bottom and
/// the DRAFT mark must sit centred. Ignored; run on demand:
///   cargo test --test hardening hf_writes_verification_artifact -- --ignored
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn hf_writes_verification_artifact() {
    let out = PathBuf::from("../Sample PDFs/vibepdf-verify-hardening.pdf");
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).expect("ensure Sample PDFs dir");
    }
    let fixture = PathBuf::from("../tests/fixtures/basic/rotated.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture, None).expect("spawn");
    handle
        .add_watermark(
            vec![0, 1, 2, 3],
            vibepdf_lib::pdf::watermark::WatermarkKind::Text {
                text: "DRAFT".into(),
                font_family: "Helvetica".into(),
                size: 64.0,
                color: "#808080".into(),
                bold: false,
                italic: false,
            },
            0.3,
            45.0,
            true,
        )
        .await
        .expect("watermark");
    handle
        .add_header_footer(
            vec![0, 1, 2, 3],
            "footer".into(),
            String::new(),
            "Page {n} of {total}".into(),
            String::new(),
            "Helvetica".into(),
            10.0,
            "#333333".into(),
            36.0,
            "2026-07-06".into(),
        )
        .await
        .expect("footer");
    handle.save(Some(out.clone())).await.expect("save");
    eprintln!("wrote hardening verification artifact to {}", out.display());
    drop(handle);
}

/// FABLE_REVIEW 3.13 (proof) — a tagged decoration is removable by a mechanical
/// operator splice: decode the content, drop the `/VibePDF BDC … EMC` range,
/// re-encode. This is the exact machinery a future "remove watermark" feature
/// will use; the test proves the rail works end to end (mark gone, original
/// content intact, document still opens in PDFium).
#[test]
fn decoration_tag_is_operator_spliceable() {
    use lopdf::content::Content;
    use vibepdf_lib::pdf::watermark::{add_watermark, WatermarkKind};

    let src = std::fs::read("../tests/fixtures/basic/hello.pdf").expect("fixture");
    let kind = WatermarkKind::Text {
        text: "DRAFT".into(),
        font_family: "Helvetica".into(),
        size: 64.0,
        color: "#808080".into(),
        bold: false,
        italic: false,
    };
    let marked = add_watermark(&src, &[0], &kind, 0.3, 45.0, true).expect("watermark");

    // Splice: find the /VibePDF BDC … EMC operator range and drop it.
    let mut doc = Document::load_mem(&marked).expect("load");
    let page_id = *doc.get_pages().get(&1).expect("page 1");
    let mut ops = doc
        .get_and_decode_page_content(page_id)
        .expect("decode content")
        .operations;
    let start = ops
        .iter()
        .position(|op| {
            op.operator == "BDC"
                && matches!(op.operands.first(), Some(Object::Name(n)) if n == b"VibePDF")
        })
        .expect("a /VibePDF BDC");
    let end = start
        + ops[start..]
            .iter()
            .position(|op| op.operator == "EMC")
            .expect("its EMC");
    ops.drain(start..=end);
    let encoded = Content { operations: ops }.encode().expect("re-encode");
    doc.change_page_content(page_id, encoded).expect("swap content");
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("save");

    // The mark is gone, the page's own content survives, and PDFium reopens it.
    let after = Document::load_mem(&out).expect("reload");
    let pid = *after.get_pages().get(&1).unwrap();
    let c = String::from_utf8_lossy(&after.get_page_content(pid).unwrap()).into_owned();
    assert!(!c.contains("DRAFT"), "the spliced-out watermark is gone; got:\n{c}");
    assert!(c.contains("Hello"), "the page's own content survives");

    let tmp = std::env::temp_dir().join(format!("vibepdf-splice-{}.pdf", uuid::Uuid::new_v4()));
    std::fs::write(&tmp, &out).expect("write temp");
    assert!(open_pdf(&tmp, None).is_ok(), "PDFium reopens the spliced document");
    let _ = std::fs::remove_file(&tmp);
}

fn encrypted_fixture() -> Option<PathBuf> {
    let p = PathBuf::from("../tests/fixtures/acceptance/p1-encrypted.pdf");
    if p.is_file() {
        Some(p)
    } else {
        eprintln!(
            "SKIP: {} missing — regenerate with `python3 tests/fixtures/acceptance/generate.py`",
            p.display()
        );
        None
    }
}

/// SPEC: P1-VIEW-003 — **pins** the save-side behavior FABLE_REVIEW 3.3 flagged
/// as unverified. Reality (discovered by this test's first run): `PDFium`
/// **preserves** the source encryption when serializing, and the round-trip
/// verification (`verify_pdf_reopens`) was re-opening the temp file with *no*
/// password — so every save of an encrypted document failed with
/// `PasswordRequired`. The P4.HF fix threads the open password into the verify.
/// Pinned here: save **succeeds**, and the saved copy is **still encrypted**
/// (no silent protection-stripping).
#[tokio::test]
async fn encrypted_open_then_save_preserves_encryption() {
    let Some(fixture) = encrypted_fixture() else { return };
    let dir = std::env::temp_dir().join(format!("vibepdf-enc-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let out = dir.join("saved.pdf");

    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture, Some("vibepdf".into()))
        .expect("open encrypted with password");
    handle
        .save(Some(out.clone()))
        .await
        .expect("saving an encrypted document must work (verify uses the open password)");
    drop(handle);

    // Still locked: no password → PasswordRequired; the right password opens it.
    let without = open_pdf(&out, None);
    assert!(
        matches!(without, Err(vibepdf_lib::error::CommandError::PasswordRequired(_))),
        "the saved copy stays encrypted (open without password must fail), got {without:?}"
    );
    let with = open_pdf(&out, Some("vibepdf"));
    assert!(with.is_ok(), "the saved copy opens with the original password");
    drop(with);

    // Belt and braces: the saved trailer still carries /Encrypt.
    let bytes = std::fs::read(&out).expect("read saved");
    let tail = &bytes[bytes.len().saturating_sub(2048)..];
    assert!(
        tail.windows(8).any(|w| w == b"/Encrypt"),
        "/Encrypt present in the saved trailer (encryption preserved)"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
