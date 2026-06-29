//! Integration tests for backgrounds (P4.D1a).
//!
//! SPEC: P4-EDIT-008 — fill selected pages with a colour or image *behind* their
//! content. Content-stream assertions are at the lopdf level; undo runs through
//! the actor. (The PDF-page source is D1b.)

use std::path::PathBuf;

use lopdf::{Document, Object};
use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::background::{add_background, BackgroundKind};

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn bytes(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).expect("read fixture")
}

fn color(hex: &str) -> BackgroundKind {
    BackgroundKind::Color(hex.to_owned())
}

/// The decoded, concatenated content of page `page_no` (1-based).
fn page_content(bytes: &[u8], page_no: u32) -> String {
    let doc = Document::load_mem(bytes).expect("load");
    let page_id = *doc.get_pages().get(&page_no).expect("page exists");
    String::from_utf8_lossy(&doc.get_page_content(page_id).expect("content")).into_owned()
}

#[test]
fn color_background_fills_behind_content() {
    // hello.pdf draws "(Hello, VibePDF.)"; the fill must precede it (prepended)
    // and paint the whole page (a `re … f`).
    let out = add_background(&bytes("hello.pdf"), &[0], &color("#ff0000"), 1.0).expect("bg");
    let c = page_content(&out, 1);
    assert!(c.contains("612.00 792.00 re"), "fills the full MediaBox");
    assert!(c.contains("1.0000 0.0000 0.0000 rg"), "uses the parsed colour");
    let (fill, orig) = (c.find("re").expect("re"), c.find("Hello").expect("orig"));
    assert!(fill < orig, "background draws before the page's own text");
}

#[test]
fn background_on_selected_pages_only() {
    // many-pages.pdf has 50 pages; fill only pages 0 and 2.
    let out = add_background(&bytes("many-pages.pdf"), &[0, 2], &color("#0000ff"), 1.0).expect("bg");
    assert!(page_content(&out, 1).contains(" re\nf"), "page 1 filled");
    assert!(page_content(&out, 3).contains(" re\nf"), "page 3 filled");
    assert!(!page_content(&out, 2).contains(" re\nf"), "page 2 untouched");
}

#[test]
fn image_background_embeds_once_and_clips() {
    let kind = BackgroundKind::Image(bytes("sample.jpg"));
    let out = add_background(&bytes("many-pages.pdf"), &[0, 1], &kind, 1.0).expect("image bg");
    let doc = Document::load_mem(&out).expect("load");
    let images = doc
        .objects
        .values()
        .filter(|o| {
            o.as_stream()
                .ok()
                .and_then(|s| s.dict.get(b"Subtype").and_then(Object::as_name).ok())
                == Some(&b"Image"[..])
        })
        .count();
    assert_eq!(images, 1, "the image is embedded once and shared");
    let c = page_content(&out, 1);
    assert!(c.contains("W n"), "clips to the page");
    assert!(c.contains(" Do"), "paints the image");
}

#[test]
fn opacity_extgstate_registered() {
    let out = add_background(&bytes("hello.pdf"), &[0], &color("#808080"), 0.5).expect("bg");
    let doc = Document::load_mem(&out).expect("load");
    let page_id = *doc.get_pages().get(&1).unwrap();
    let res = doc.get_dictionary(page_id).unwrap().get(b"Resources").unwrap().as_dict().unwrap();
    let egs = res.get(b"ExtGState").unwrap().as_dict().unwrap();
    let gs = egs.get(b"GSbg").unwrap().as_dict().unwrap();
    let ca = gs.get(b"ca").unwrap().as_float().unwrap();
    assert!((ca - 0.5).abs() < 1e-4, "opacity {ca} ~ 0.5");
}

#[test]
fn empty_pages_errors() {
    assert!(add_background(&bytes("hello.pdf"), &[], &color("#000000"), 1.0).is_err());
}

#[test]
fn bad_color_errors() {
    assert!(add_background(&bytes("hello.pdf"), &[0], &color("blue"), 1.0).is_err());
}

#[test]
fn page_out_of_range_errors() {
    assert!(add_background(&bytes("hello.pdf"), &[5], &color("#000000"), 1.0).is_err());
}

#[tokio::test]
async fn actor_background_then_undo() {
    let dir = std::env::temp_dir().join(format!("vibepdf-bg-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let out = dir.join("bg.pdf");

    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    let state = handle.add_background(vec![0], color("#e6f0ff"), 1.0).await.expect("bg");
    assert!(state.can_undo, "a background must be undoable");

    handle.save(Some(out.clone())).await.expect("save");
    assert!(page_content(&std::fs::read(&out).unwrap(), 1).contains(" re\nf"), "filled after save");

    handle.undo().await.expect("undo");
    handle.save(Some(out.clone())).await.expect("save after undo");
    assert!(!page_content(&std::fs::read(&out).unwrap(), 1).contains(" re\nf"), "undo removes it");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Writes a backgrounded PDF to the git-ignored `Sample PDFs/` for the manual
/// cross-reader ritual. Ignored; run on demand:
///   cargo test --test background bg_writes_verification_artifact -- --ignored
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn bg_writes_verification_artifact() {
    let out = PathBuf::from("../Sample PDFs/vibepdf-verify-background.pdf");
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).expect("ensure Sample PDFs dir");
    }
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");
    handle.add_background(vec![0], color("#e6f0ff"), 1.0).await.expect("bg");
    handle.save(Some(out.clone())).await.expect("save");
    eprintln!("wrote background verification artifact to {}", out.display());

    drop(handle);
}
