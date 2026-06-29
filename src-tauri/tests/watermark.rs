//! Integration tests for watermarks (P4.D2).
//!
//! SPEC: P4-EDIT-009 — stamp a text or image watermark on selected pages, on top
//! of or behind content, with opacity + rotation. Content-stream assertions are
//! at the lopdf level; undo runs through the actor. The 50-page acceptance
//! (`<2s`) runs against the `many-pages.pdf` fixture.

use std::path::PathBuf;
use std::time::Instant;

use lopdf::{Document, Object, ObjectId};
use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::watermark::{add_watermark, WatermarkKind};

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn bytes(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).expect("read fixture")
}

fn text_kind(text: &str) -> WatermarkKind {
    WatermarkKind::Text {
        text: text.to_owned(),
        font_family: "Helvetica".to_owned(),
        size: 64.0,
        color: "#808080".to_owned(),
        bold: false,
        italic: false,
    }
}

/// The decoded, concatenated content of page `page_no` (1-based) as a string.
fn page_content(bytes: &[u8], page_no: u32) -> String {
    let doc = Document::load_mem(bytes).expect("load");
    let page_id = *doc.get_pages().get(&page_no).expect("page exists");
    let content = doc.get_page_content(page_id).expect("content");
    String::from_utf8_lossy(&content).into_owned()
}

/// The page's `/ExtGState /GSwm` `/ca` opacity, if our watermark registered one.
fn watermark_opacity(bytes: &[u8], page_no: u32) -> Option<f32> {
    let doc = Document::load_mem(bytes).expect("load");
    let page_id = *doc.get_pages().get(&page_no)?;
    let res = doc.get_dictionary(page_id).ok()?.get(b"Resources").ok()?;
    let res = match res {
        Object::Dictionary(d) => d.clone(),
        Object::Reference(id) => doc.get_dictionary(*id).ok()?.clone(),
        _ => return None,
    };
    let egs = res.get(b"ExtGState").and_then(Object::as_dict).ok()?;
    let gs = egs.get(b"GSwm").ok()?;
    let gs = match gs {
        Object::Dictionary(d) => d.clone(),
        Object::Reference(id) => doc.get_dictionary(*id).ok()?.clone(),
        _ => return None,
    };
    gs.get(b"ca").and_then(Object::as_float).ok()
}

#[test]
fn text_watermark_on_selected_pages_only() {
    // many-pages.pdf has 50 pages; stamp only pages 0 and 2 (1-based 1 and 3).
    let out = add_watermark(&bytes("many-pages.pdf"), &[0, 2], &text_kind("DRAFT"), 0.3, 45.0, true)
        .expect("watermark");
    assert!(page_content(&out, 1).contains("(DRAFT) Tj"), "page 1 stamped");
    assert!(page_content(&out, 3).contains("(DRAFT) Tj"), "page 3 stamped");
    assert!(!page_content(&out, 2).contains("(DRAFT) Tj"), "page 2 untouched");
    assert!(!page_content(&out, 4).contains("(DRAFT) Tj"), "page 4 untouched");
}

#[test]
fn behind_prepends_on_top_appends() {
    // hello.pdf's own text draws "(Hello, VibePDF.)"; our watermark marker is /GSwm.
    let behind = add_watermark(&bytes("hello.pdf"), &[0], &text_kind("WM"), 0.3, 0.0, true).expect("behind");
    let c = page_content(&behind, 1);
    let (wm, orig) = (c.find("GSwm").expect("wm"), c.find("Hello").expect("orig"));
    assert!(wm < orig, "behind: watermark draws before the page text");

    let ontop = add_watermark(&bytes("hello.pdf"), &[0], &text_kind("WM"), 0.3, 0.0, false).expect("ontop");
    let c = page_content(&ontop, 1);
    let (wm, orig) = (c.find("GSwm").expect("wm"), c.find("Hello").expect("orig"));
    assert!(wm > orig, "on top: watermark draws after the page text");
}

#[test]
fn opacity_extgstate_registered() {
    let out = add_watermark(&bytes("hello.pdf"), &[0], &text_kind("DRAFT"), 0.25, 45.0, true).expect("wm");
    let op = watermark_opacity(&out, 1).expect("an /ExtGState /GSwm /ca");
    assert!((op - 0.25).abs() < 1e-4, "opacity {op} ~ 0.25");
}

#[test]
fn rotation_matrix_emitted() {
    // 45° → cos == sin == 0.70711 (formatted to 5dp).
    let out = add_watermark(&bytes("hello.pdf"), &[0], &text_kind("DRAFT"), 0.3, 45.0, true).expect("wm");
    assert!(page_content(&out, 1).contains("0.70711 0.70711"), "rotation cm present");
}

#[test]
fn image_watermark_embeds_once() {
    // Two pages, one shared image XObject.
    let kind = WatermarkKind::Image(bytes("sample.jpg"));
    let out = add_watermark(&bytes("many-pages.pdf"), &[0, 1], &kind, 0.4, 30.0, false).expect("image wm");
    let doc = Document::load_mem(&out).expect("load");
    let image_xobjects = doc
        .objects
        .iter()
        .filter(|(_, o)| {
            o.as_stream()
                .ok()
                .and_then(|s| s.dict.get(b"Subtype").and_then(Object::as_name).ok())
                == Some(&b"Image"[..])
        })
        .count();
    assert_eq!(image_xobjects, 1, "the image is embedded once and shared");
    assert!(page_content(&out, 1).contains(" Do"), "page 1 paints it");
    assert!(page_content(&out, 2).contains(" Do"), "page 2 paints it");
}

#[test]
fn empty_pages_errors() {
    let err = add_watermark(&bytes("hello.pdf"), &[], &text_kind("X"), 0.3, 0.0, true);
    assert!(err.is_err(), "no pages selected must error");
}

#[test]
fn empty_text_errors() {
    let err = add_watermark(&bytes("hello.pdf"), &[0], &text_kind("   "), 0.3, 0.0, true);
    assert!(err.is_err(), "blank watermark text must error");
}

#[test]
fn page_out_of_range_errors() {
    let err = add_watermark(&bytes("hello.pdf"), &[5], &text_kind("X"), 0.3, 0.0, true);
    assert!(err.is_err(), "out-of-range page must error");
}

/// SPEC: P4-EDIT-009 (acceptance) — "DRAFT" on a 50-page PDF in under 2s.
#[test]
fn fifty_page_watermark_under_2s() {
    let src = bytes("many-pages.pdf");
    let pages: Vec<usize> = (0..50).collect();
    let start = Instant::now();
    let out = add_watermark(&src, &pages, &text_kind("DRAFT"), 0.3, 45.0, true).expect("wm");
    let elapsed = start.elapsed();
    // Every page stamped.
    let doc = Document::load_mem(&out).expect("load");
    let stamped = (1..=50u32)
        .filter(|&n| {
            let id: ObjectId = *doc.get_pages().get(&n).unwrap();
            String::from_utf8_lossy(&doc.get_page_content(id).unwrap()).contains("(DRAFT) Tj")
        })
        .count();
    assert_eq!(stamped, 50, "all 50 pages stamped");
    assert!(elapsed.as_secs_f32() < 2.0, "50-page watermark took {elapsed:?} (budget 2s)");
}

#[tokio::test]
async fn actor_watermark_then_undo() {
    let dir = std::env::temp_dir().join(format!("vibepdf-wm-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let out = dir.join("wm.pdf");

    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    let state = handle
        .add_watermark(vec![0], text_kind("DRAFT"), 0.3, 45.0, true)
        .await
        .expect("watermark");
    assert!(state.can_undo, "a watermark must be undoable");

    handle.save(Some(out.clone())).await.expect("save");
    assert!(page_content(&std::fs::read(&out).unwrap(), 1).contains("(DRAFT) Tj"), "stamped after save");

    handle.undo().await.expect("undo");
    handle.save(Some(out.clone())).await.expect("save after undo");
    assert!(!page_content(&std::fs::read(&out).unwrap(), 1).contains("(DRAFT) Tj"), "undo removes it");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Writes a watermarked PDF to the git-ignored `Sample PDFs/` for the manual
/// cross-reader ritual. Ignored; run on demand:
///   cargo test --test watermark watermark_writes_verification_artifact -- --ignored
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn watermark_writes_verification_artifact() {
    let out = PathBuf::from("../Sample PDFs/vibepdf-verify-watermark.pdf");
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).expect("ensure Sample PDFs dir");
    }
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("many-pages.pdf"), None).expect("spawn");
    let pages: Vec<i32> = (0..50).collect();
    handle
        .add_watermark(pages, text_kind("DRAFT"), 0.3, 45.0, true)
        .await
        .expect("watermark");
    handle.save(Some(out.clone())).await.expect("save");
    eprintln!("wrote watermark verification artifact to {}", out.display());

    drop(handle);
}
