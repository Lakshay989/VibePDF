//! Integration tests for adding an image as page content (P4.C1).
//!
//! SPEC: P4-EDIT-005 — embed an image (PNG → raw samples, JPEG → DCTDecode) into
//! the page **content stream** (an `XObject` referenced by `Do`), not as an
//! annotation. Resize/position is the aspect-fit into the drawn box.

use std::path::PathBuf;

use lopdf::Document;

use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::cos::add_image;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

/// Encode a tiny RGB PNG in-memory (the `png` crate is a dependency).
fn make_png(w: u32, h: u32) -> Vec<u8> {
    let data = vec![140u8; (w * h) as usize * 3];
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().unwrap();
        writer.write_image_data(&data).unwrap();
    }
    out
}

const RECT: [f32; 4] = [100.0, 400.0, 300.0, 600.0];

/// Reopen bytes in PDFium (the actor) and return the page count — proves the edit
/// is valid to the other engine.
async fn pdfium_page_count(bytes: &[u8]) -> u32 {
    let p = std::env::temp_dir().join(format!("vibepdf-img-{}.pdf", uuid::Uuid::new_v4()));
    std::fs::write(&p, bytes).expect("write temp");
    let handle = DocumentActorHandle::spawn(None, uuid::Uuid::new_v4(), p, None).expect("spawn");
    let n = handle.page_count().await.expect("page count");
    drop(handle);
    n
}

async fn page0_annot_count(bytes: &[u8]) -> usize {
    let p = std::env::temp_dir().join(format!("vibepdf-img-{}.pdf", uuid::Uuid::new_v4()));
    std::fs::write(&p, bytes).expect("write temp");
    let handle = DocumentActorHandle::spawn(None, uuid::Uuid::new_v4(), p, None).expect("spawn");
    let annots = handle.read_annotations().await.expect("annots");
    drop(handle);
    annots.iter().filter(|a| a.page == 0).count()
}

/// Find the first page's first `/XObject` resource stream (the embedded image).
fn first_xobject_stream(bytes: &[u8]) -> lopdf::Stream {
    let doc = Document::load_mem(bytes).expect("load");
    let page_id = *doc.get_pages().get(&1).expect("page 1");
    let resources = doc.get_dictionary(page_id).unwrap().get(b"Resources").unwrap().as_dict().unwrap();
    let xobjects = resources.get(b"XObject").unwrap().as_dict().unwrap();
    let (_name, obj) = xobjects.iter().next().expect("an XObject");
    let id = obj.as_reference().unwrap();
    doc.get_object(id).unwrap().as_stream().unwrap().clone()
}

/// Does any page-content stream reference an XObject via `Do`?
fn content_has_do(bytes: &[u8]) -> bool {
    let doc = Document::load_mem(bytes).expect("load");
    let page_id = *doc.get_pages().get(&1).expect("page 1");
    let content = doc.get_page_content(page_id).expect("content");
    String::from_utf8_lossy(&content).contains(" Do")
}

#[tokio::test]
async fn embeds_png_as_content_xobject_not_annotation() {
    let original = std::fs::read(fixture("hello.pdf")).expect("read hello.pdf");
    let before_annots = page0_annot_count(&original).await;

    let edited = add_image(&original, 0, RECT, &make_png(4, 3)).expect("add image");

    // The image is a content-stream XObject painted by `Do` — not an annotation.
    let stream = first_xobject_stream(&edited);
    assert_eq!(stream.dict.get(b"Subtype").unwrap().as_name().unwrap(), b"Image");
    assert!(content_has_do(&edited), "page content paints the image with Do");
    assert_eq!(page0_annot_count(&edited).await, before_annots, "no annotation added");
    assert_eq!(pdfium_page_count(&edited).await, 1, "round-trips through PDFium");
}

#[tokio::test]
async fn embeds_jpeg_with_dctdecode() {
    let original = std::fs::read(fixture("hello.pdf")).expect("read hello.pdf");
    let jpeg = std::fs::read(fixture("sample.jpg")).expect("read sample.jpg");

    let edited = add_image(&original, 0, RECT, &jpeg).expect("add jpeg");
    let stream = first_xobject_stream(&edited);
    assert_eq!(stream.dict.get(b"Filter").unwrap().as_name().unwrap(), b"DCTDecode");
    assert!(stream.dict.get(b"Width").unwrap().as_i64().unwrap() > 0, "parsed JPEG width");
    assert_eq!(pdfium_page_count(&edited).await, 1, "JPEG content round-trips through PDFium");
}

#[tokio::test]
async fn unique_xobject_name_no_collision() {
    // Two adds onto the same page must produce two distinct XObject resource names.
    let original = std::fs::read(fixture("hello.pdf")).expect("read hello.pdf");
    let once = add_image(&original, 0, RECT, &make_png(4, 3)).expect("add 1");
    let twice = add_image(&once, 0, [50.0, 50.0, 150.0, 150.0], &make_png(2, 2)).expect("add 2");

    let doc = Document::load_mem(&twice).expect("load");
    let page_id = *doc.get_pages().get(&1).unwrap();
    let resources = doc.get_dictionary(page_id).unwrap().get(b"Resources").unwrap().as_dict().unwrap();
    let xobjects = resources.get(b"XObject").unwrap().as_dict().unwrap();
    assert_eq!(xobjects.len(), 2, "two images → two distinct resource names: {xobjects:?}");
    assert_eq!(pdfium_page_count(&twice).await, 1, "two images round-trip");
}

#[tokio::test]
async fn unsupported_format_errors() {
    let original = std::fs::read(fixture("hello.pdf")).expect("read hello.pdf");
    assert!(add_image(&original, 0, RECT, b"GIF89a\x01\x00\x01\x00").is_err(), "GIF is rejected");
    assert!(add_image(&original, 0, RECT, b"not an image").is_err(), "garbage is rejected");
    assert!(add_image(&original, 0, [0.0, 0.0, 0.0, 0.0], &make_png(2, 2)).is_err(), "empty rect rejected");
}

#[tokio::test]
async fn actor_add_then_undo() {
    let handle = DocumentActorHandle::spawn(None, uuid::Uuid::new_v4(), fixture("hello.pdf"), None).expect("spawn");

    let state = handle.add_image(0, RECT, make_png(4, 3)).await.expect("add");
    assert!(state.can_undo, "adding an image is undoable");
    let bytes = handle.get_bytes().await.expect("bytes");
    assert!(content_has_do(&bytes), "image present after actor add");

    handle.undo().await.expect("undo");
    let after = handle.get_bytes().await.expect("bytes");
    assert!(!content_has_do(&after), "undo removes the image");
    drop(handle);
}

#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn writes_verification_artifact() {
    let original = std::fs::read(fixture("hello.pdf")).expect("read hello.pdf");
    let edited = add_image(&original, 0, [180.0, 480.0, 432.0, 640.0], &make_png(64, 48)).expect("add");
    std::fs::write("/tmp/vibepdf-verify.pdf", &edited).expect("write artifact");
}
