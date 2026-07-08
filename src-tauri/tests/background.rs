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

// --- P4.D1b: a page from another PDF as the background --------------------------

/// Every Form `XObject` stream in `bytes`.
fn form_xobjects(bytes: &[u8]) -> Vec<lopdf::Stream> {
    let doc = Document::load_mem(bytes).expect("load");
    doc.objects
        .values()
        .filter_map(|o| o.as_stream().ok())
        .filter(|s| s.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(&b"Form"[..]))
        .cloned()
        .collect()
}

fn pdf_page(source: &str, page: usize) -> BackgroundKind {
    BackgroundKind::PdfPage { source: bytes(source), page }
}

fn object_count(bytes: &[u8]) -> usize {
    Document::load_mem(bytes).expect("load").objects.len()
}

#[test]
fn pdf_page_background_imports_form() {
    // links.pdf page 1 behind hello.pdf.
    let out = add_background(&bytes("hello.pdf"), &[0], &pdf_page("links.pdf", 0), 1.0).expect("pdf bg");
    assert_eq!(form_xobjects(&out).len(), 1, "the source page becomes one Form XObject");
    let c = page_content(&out, 1);
    let (form, orig) = (c.find("Bgpdf").expect("Do"), c.find("Hello").expect("orig"));
    assert!(form < orig, "the imported page draws behind hello's own text");
}

#[test]
fn pdf_background_copies_page_resources_and_content() {
    let out = add_background(&bytes("hello.pdf"), &[0], &pdf_page("links.pdf", 0), 1.0).expect("pdf bg");
    let form = &form_xobjects(&out)[0];
    let res = form.dict.get(b"Resources").and_then(Object::as_dict).expect("form resources");
    assert!(res.has(b"Font"), "the source page's /Font is copied into the Form's /Resources");
    let stream = String::from_utf8_lossy(&form.content);
    assert!(stream.contains("Tj"), "the source page's content (a text show) is the Form's stream");
}

#[test]
fn pdf_background_embeds_once() {
    // links.pdf (3 pages) as target; hello.pdf page 1 as the shared source.
    let out = add_background(&bytes("links.pdf"), &[0, 1, 2], &pdf_page("hello.pdf", 0), 1.0).expect("pdf bg");
    assert_eq!(form_xobjects(&out).len(), 1, "one Form, referenced from every page");
    for p in 1..=3 {
        assert!(page_content(&out, p).contains("Bgpdf Do"), "page {p} paints it");
    }
}

#[test]
fn pdf_background_copies_only_the_page_subtree_not_whole_source() {
    // The subtree copy must add far fewer objects than absorbing all of links.pdf.
    let out = add_background(&bytes("hello.pdf"), &[0], &pdf_page("links.pdf", 0), 1.0).expect("pdf bg");
    let (hello_n, links_n, out_n) =
        (object_count(&bytes("hello.pdf")), object_count(&bytes("links.pdf")), object_count(&out));
    assert!(out_n < hello_n + links_n, "out={out_n} must be < hello={hello_n} + links={links_n} (no whole-doc copy)");
}

#[test]
fn source_page_out_of_range_errors() {
    // hello.pdf has 1 page; source page index 5 is out of range.
    assert!(add_background(&bytes("hello.pdf"), &[0], &pdf_page("hello.pdf", 5), 1.0).is_err());
}

#[tokio::test]
async fn actor_pdf_background_then_undo() {
    let dir = std::env::temp_dir().join(format!("vibepdf-pdfbg-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let out = dir.join("pdfbg.pdf");

    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    let state = handle.add_background(vec![0], pdf_page("links.pdf", 0), 1.0).await.expect("pdf bg");
    assert!(state.can_undo, "a PDF-page background must be undoable");

    handle.save(Some(out.clone())).await.expect("save");
    assert_eq!(form_xobjects(&std::fs::read(&out).unwrap()).len(), 1, "Form present after save");

    handle.undo().await.expect("undo");
    handle.save(Some(out.clone())).await.expect("save after undo");
    assert!(form_xobjects(&std::fs::read(&out).unwrap()).is_empty(), "undo removes the imported Form");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

// --- P4.HF: rotation + CropBox compensation ------------------------------------

/// SPEC: P4-EDIT-008 — an image background covers the *visual* box of a rotated
/// page (swapped dims), clipped there, with the compensating `cm`.
#[test]
fn image_background_covers_visual_box_on_rotated_page() {
    let kind = BackgroundKind::Image(bytes("sample.jpg"));
    let out = add_background(&bytes("rotated.pdf"), &[1], &kind, 1.0).expect("image bg on /Rotate 90");
    let c = page_content(&out, 2);
    assert!(
        c.contains("0.00000 1.00000 -1.00000 0.00000 612.00 0.00 cm"),
        "compensating cm for /Rotate 90; got:\n{c}"
    );
    assert!(c.contains("0.00 0.00 792.00 612.00 re"), "clips to the visual (swapped) box; got:\n{c}");
}

/// SPEC: P4-EDIT-008 — the colour fill still covers the full MediaBox on a
/// cropped page (bleed-safe), while placement-sensitive kinds go visual.
#[test]
fn color_fill_still_covers_mediabox_on_cropped_page() {
    let out = add_background(&bytes("cropped.pdf"), &[0], &color("#ff0000"), 1.0).expect("bg");
    let c = page_content(&out, 1);
    assert!(c.contains("0.00 0.00 612.00 792.00 re"), "fills the MediaBox, not the crop; got:\n{c}");
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
    // Two backgrounds to exercise both D1a (colour) and D1b (a PDF page): a faint
    // colour wash, then links.pdf page 1 behind it. Both behind hello's text.
    handle.add_background(vec![0], color("#e6f0ff"), 1.0).await.expect("colour bg");
    handle.add_background(vec![0], pdf_page("links.pdf", 0), 0.35).await.expect("pdf bg");
    handle.save(Some(out.clone())).await.expect("save");
    eprintln!("wrote background verification artifact to {}", out.display());

    drop(handle);
}
