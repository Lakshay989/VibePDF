//! Integration tests for stamp annotations (P3.C3a).
//!
//! SPEC: P3-ANN-006 — place a `/Stamp` with a generated `/AP`, persisted through
//! the PDFium save round-trip and undoable. Exercised through the actor against
//! `hello.pdf`.

use std::path::{Path, PathBuf};

use lopdf::{Dictionary, Document, Object};
use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::cos::add_image_stamp;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).expect("read fixture")
}

/// A tiny `width`×`height` PNG of the given colour type, filled with mid-gray.
fn make_png(width: u32, height: u32, color: png::ColorType) -> Vec<u8> {
    let components = match color {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        _ => unreachable!(),
    };
    let data = vec![128u8; (width * height) as usize * components];
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, width, height);
        enc.set_color(color);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().expect("header");
        w.write_image_data(&data).expect("data");
    }
    out
}

/// The dictionary of the first Image XObject in `doc`, if any.
fn first_image_xobject(doc: &Document) -> Option<&Dictionary> {
    doc.objects.values().find_map(|o| {
        let s = o.as_stream().ok()?;
        (s.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(&b"Image"[..])).then_some(&s.dict)
    })
}

/// The first `/Stamp` annotation's `/Rect` `[x0,y0,x1,y1]` on page 1.
fn first_stamp_rect(doc: &Document) -> Option<[f32; 4]> {
    let page_id = *doc.get_pages().get(&1)?;
    let arr = match doc.get_dictionary(page_id).ok().and_then(|p| p.get(b"Annots").ok().cloned()) {
        Some(Object::Array(a)) => a,
        Some(Object::Reference(id)) => doc.get_object(id).and_then(Object::as_array).cloned().unwrap_or_default(),
        _ => return None,
    };
    let dict = arr
        .iter()
        .filter_map(|o| o.as_reference().ok())
        .filter_map(|id| doc.get_dictionary(id).ok())
        .find(|d| d.get(b"Subtype").and_then(Object::as_name).ok() == Some(&b"Stamp"[..]))?;
    let r = dict.get(b"Rect").and_then(Object::as_array).ok()?;
    let n = |i: usize| match r.get(i) {
        Some(Object::Real(v)) => *v,
        Some(Object::Integer(v)) => *v as f32,
        _ => 0.0,
    };
    Some([n(0), n(1), n(2), n(3)])
}

fn temp_subdir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("vibepdf-stamp-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

fn count_subtype(path: &Path, wanted: &[u8]) -> usize {
    let bytes = std::fs::read(path).expect("read");
    let doc = Document::load_mem(&bytes).expect("load");
    let Some(&page_id) = doc.get_pages().get(&1) else {
        return 0;
    };
    let arr = match doc.get_dictionary(page_id).ok().and_then(|p| p.get(b"Annots").ok().cloned()) {
        Some(Object::Array(a)) => a,
        Some(Object::Reference(id)) => {
            doc.get_object(id).and_then(Object::as_array).cloned().unwrap_or_default()
        }
        _ => return 0,
    };
    arr.iter()
        .filter_map(|o| o.as_reference().ok())
        .filter_map(|id| doc.get_dictionary(id).ok())
        .filter(|d| d.get(b"Subtype").and_then(Object::as_name).ok() == Some(wanted))
        .count()
}

#[tokio::test]
async fn stamp_persists_through_save() {
    let dir = temp_subdir();
    let out = dir.join("stamp.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    let state = handle
        .add_stamp(0, [100.0, 600.0, 250.0, 646.0], "APPROVED".into(), "Approved".into(), "#1e8449".into(), 1.0)
        .await
        .expect("stamp");
    assert!(state.can_undo, "a stamp must be undoable");

    handle.save(Some(out.clone())).await.expect("save");
    assert_eq!(count_subtype(&out, b"Stamp"), 1, "the /Stamp survives the PDFium round-trip");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn stamp_undo_removes_it() {
    let dir = temp_subdir();
    let out = dir.join("undo.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    handle
        .add_stamp(0, [100.0, 600.0, 250.0, 646.0], "DRAFT".into(), "Draft".into(), "#555555".into(), 1.0)
        .await
        .expect("add");
    let after_undo = handle.undo().await.expect("undo");
    assert!(after_undo.can_redo, "undo of add-stamp enables redo");

    handle.save(Some(out.clone())).await.expect("save");
    assert_eq!(count_subtype(&out, b"Stamp"), 0, "undo removed the stamp");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

// SPEC: P3-ANN-006 (P3.C3b) — pure cos: an RGB PNG embeds as an Image XObject,
// the /Stamp carries an /AP, and the rect follows the image's aspect ratio.
#[test]
fn image_stamp_embeds_png_with_aspect_rect() {
    // An 8×4 image (2:1) placed at height 64 → a ~128×64 rect.
    let png = make_png(8, 4, png::ColorType::Rgb);
    let out = add_image_stamp(&fixture_bytes("hello.pdf"), 0, 300.0, 400.0, 64.0, &png, None, 1.0)
        .expect("image stamp");
    let doc = Document::load_mem(&out).expect("load");

    let img = first_image_xobject(&doc).expect("an Image XObject is embedded");
    assert_eq!(img.get(b"ColorSpace").unwrap().as_name().unwrap(), b"DeviceRGB");
    assert!(img.get(b"SMask").is_err(), "opaque RGB → no soft mask");

    let [x0, y0, x1, y1] = first_stamp_rect(&doc).expect("a /Stamp rect");
    let (w, h) = (x1 - x0, y1 - y0);
    assert!((h - 64.0).abs() < 0.5, "height honoured: {h}");
    assert!((w / h - 2.0).abs() < 0.05, "aspect 2:1 preserved (not stretched): {w}×{h}");
}

// SPEC: P3-ANN-006 (P3.C3b) — an RGBA PNG splits its alpha into an /SMask.
#[test]
fn image_stamp_alpha_writes_smask() {
    let png = make_png(4, 4, png::ColorType::Rgba);
    let out = add_image_stamp(&fixture_bytes("hello.pdf"), 0, 100.0, 100.0, 48.0, &png, None, 1.0)
        .expect("image stamp");
    let doc = Document::load_mem(&out).expect("load");
    // Two Image XObjects: the colour image (which references) + the gray soft mask.
    let has_smask = doc.objects.values().any(|o| {
        o.as_stream().ok().is_some_and(|s| {
            s.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(&b"Image"[..])
                && s.dict.get(b"SMask").and_then(Object::as_reference).is_ok()
        })
    });
    assert!(has_smask, "alpha → an Image XObject carrying an /SMask reference");
}

// SPEC: P3-ANN-006 (P3.C3b) — an image+text stamp carries the label as /Contents.
#[test]
fn image_stamp_with_text_sets_contents() {
    let png = make_png(4, 4, png::ColorType::Rgb);
    let out = add_image_stamp(&fixture_bytes("hello.pdf"), 0, 100.0, 100.0, 48.0, &png, Some("DRAFT"), 1.0)
        .expect("image stamp");
    let infos = vibepdf_lib::pdf::cos::read_annotations(&out).expect("read");
    assert_eq!(infos.len(), 1);
    assert_eq!(infos[0].kind, "stamp", "an image stamp reads back as a stamp");
    assert_eq!(infos[0].contents, "DRAFT");
}

// P4.HF10: a non-WinAnsi image-stamp label embeds a CID font into the /AP.
#[test]
fn image_stamp_unicode_label_embeds_cid_font() {
    let png = make_png(4, 4, png::ColorType::Rgb);
    let label = "\u{041F}\u{0440}\u{043E}\u{0432}"; // Пров
    let r = add_image_stamp(&fixture_bytes("hello.pdf"), 0, 100.0, 100.0, 48.0, &png, Some(label), 1.0);
    if vibepdf_lib::pdf::font_resolver::covering_font_bytes(&label.to_uppercase()).is_some() {
        let out = r.expect("image stamp embeds a covering font");
        assert!(out.windows(6).any(|w| w == b"/Type0"), "image-stamp label embeds a CID font");
    } else {
        assert!(r.is_err(), "with no covering font, the non-WinAnsi label is rejected");
    }
}

#[test]
fn image_stamp_rejects_non_png() {
    let err = add_image_stamp(&fixture_bytes("hello.pdf"), 0, 100.0, 100.0, 48.0, b"\xff\xd8 not a png", None, 1.0);
    assert!(err.is_err(), "a non-PNG image is rejected, not silently mis-embedded");
}

// SPEC: P3-ANN-006 (P3.C3b) — through the actor: an image stamp survives the
// PDFium round-trip and is undoable.
#[tokio::test]
async fn image_stamp_persists_and_undoes() {
    let dir = temp_subdir();
    let out = dir.join("image-stamp.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    let png = make_png(6, 4, png::ColorType::Rgba);
    let state = handle
        .add_image_stamp(0, 300.0, 400.0, 64.0, png, None, 1.0)
        .await
        .expect("image stamp");
    assert!(state.can_undo, "an image stamp is undoable");

    handle.save(Some(out.clone())).await.expect("save");
    assert_eq!(count_subtype(&out, b"Stamp"), 1, "the image /Stamp survives the round-trip");

    handle.undo().await.expect("undo");
    let bytes = handle.get_bytes().await.expect("bytes");
    let doc = Document::load_mem(&bytes).expect("load");
    assert!(first_stamp_rect(&doc).is_none(), "undo removed the image stamp");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn stamp_rejects_empty_text() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");
    let err = handle
        .add_stamp(0, [10.0, 10.0, 200.0, 56.0], "  ".into(), "Draft".into(), "#000000".into(), 1.0)
        .await;
    assert!(err.is_err(), "a blank stamp label is rejected");
    drop(handle);
}

/// Writes a stamp PDF to the git-ignored `Sample PDFs/` for the manual
/// cross-reader ritual. Ignored; run on demand:
///   cargo test --test stamp stamp_writes_verification_artifact -- --ignored
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn stamp_writes_verification_artifact() {
    let out = PathBuf::from("../Sample PDFs/vibepdf-verify-stamp.pdf");
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).expect("ensure Sample PDFs dir");
    }
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    // A built-in + a custom stamp, different colours.
    handle
        .add_stamp(0, [80.0, 680.0, 230.0, 726.0], "APPROVED".into(), "Approved".into(), "#1e8449".into(), 1.0)
        .await
        .expect("approved");
    handle
        .add_stamp(0, [80.0, 600.0, 320.0, 650.0], "CONFIDENTIAL".into(), "Confidential".into(), "#c0392b".into(), 0.85)
        .await
        .expect("confidential");
    // An image stamp (a 2:1 RGBA PNG) + an image+text combination, to eyeball the
    // C3b path cross-reader (aspect, transparency, overlaid label).
    handle
        .add_image_stamp(0, 200.0, 500.0, 60.0, make_png(64, 32, png::ColorType::Rgba), None, 1.0)
        .await
        .expect("image stamp");
    handle
        .add_image_stamp(0, 200.0, 420.0, 60.0, make_png(64, 32, png::ColorType::Rgb), Some("SIGNED".into()), 0.9)
        .await
        .expect("image+text stamp");
    handle.save(Some(out.clone())).await.expect("save");
    eprintln!("wrote stamp verification artifact to {}", out.display());

    drop(handle);
}

/// P4.HF10: non-WinAnsi stamp labels embed a CID font into the /AP.
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn stamp_embedded_unicode_artifact() {
    let out = PathBuf::from("../Sample PDFs/vibepdf-verify-stamp-unicode.pdf");
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).expect("ensure Sample PDFs dir");
    }
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");
    handle
        .add_stamp(0, [80.0, 660.0, 320.0, 710.0], "Утверждено".into(), "Approved".into(), "#1e8449".into(), 1.0)
        .await
        .expect("cyrillic text stamp");
    handle
        .add_image_stamp(0, 200.0, 520.0, 70.0, make_png(64, 32, png::ColorType::Rgba), Some("Εγκρίθηκε".into()), 0.9)
        .await
        .expect("greek image+label stamp");
    handle.save(Some(out.clone())).await.expect("save");
    eprintln!("wrote embedded-Unicode stamp artifact to {}", out.display());

    drop(handle);
}
