//! Integration tests for crop (P2.B4).
//!
//! SPEC: P2-PAGE-009 — adjust the `/CropBox` (content untouched), reset to
//! the `/MediaBox`, undoable. Exercised through the actor against
//! `links.pdf` (3 US-Letter pages, no explicit CropBox → CropBox defaults
//! to the MediaBox 0,0,612,792).

use std::path::{Path, PathBuf};

use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::document::open_pdf;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn temp_subdir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("vibepdf-crop-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

/// (left, bottom, right, top) of `page`'s CropBox, in points.
fn crop_box(path: &Path, page: i32) -> (f32, f32, f32, f32) {
    let (doc, _m) = open_pdf(path, None).expect("open for crop read");
    let r = {
        let p = doc.pages().get(page).expect("get page");
        let b = p
            .boundaries()
            .crop()
            .or_else(|_| p.boundaries().media())
            .expect("crop/media box")
            .bounds;
        (b.left().value, b.bottom().value, b.right().value, b.top().value)
    };
    drop(doc);
    r
}

fn approx4(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> bool {
    (a.0 - b.0).abs() < 1.0
        && (a.1 - b.1).abs() < 1.0
        && (a.2 - b.2).abs() < 1.0
        && (a.3 - b.3).abs() < 1.0
}

#[tokio::test]
async fn crop_sets_cropbox_and_persists() {
    let dir = temp_subdir();
    let out = dir.join("cropped.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("links.pdf"), None).expect("spawn");

    // Inset 50pt on every edge of the 612×792 page.
    let rect = (50.0, 50.0, 562.0, 742.0);
    let state = handle.crop_page(0, Some(rect)).await.expect("crop");
    assert!(state.can_undo, "a crop must be undoable");

    handle.save(Some(out.clone())).await.expect("save-as");
    assert!(
        approx4(crop_box(&out, 0), rect),
        "got {:?}, want {rect:?}",
        crop_box(&out, 0)
    );

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn reset_restores_mediabox() {
    let dir = temp_subdir();
    let out = dir.join("reset.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("links.pdf"), None).expect("spawn");

    handle.crop_page(0, Some((50.0, 50.0, 562.0, 742.0))).await.expect("crop");
    handle.crop_page(0, None).await.expect("reset"); // back to MediaBox

    handle.save(Some(out.clone())).await.expect("save-as");
    assert!(
        approx4(crop_box(&out, 0), (0.0, 0.0, 612.0, 792.0)),
        "reset should restore the full MediaBox, got {:?}",
        crop_box(&out, 0)
    );

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn undo_restores_previous_box() {
    let dir = temp_subdir();
    let out = dir.join("undo.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("links.pdf"), None).expect("spawn");

    handle.crop_page(0, Some((50.0, 50.0, 562.0, 742.0))).await.expect("crop");
    let after_undo = handle.undo().await.expect("undo");
    assert!(after_undo.can_redo);

    handle.save(Some(out.clone())).await.expect("save-as");
    assert!(
        approx4(crop_box(&out, 0), (0.0, 0.0, 612.0, 792.0)),
        "undo should restore the original box, got {:?}",
        crop_box(&out, 0)
    );

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn crop_out_of_range_is_typed_error() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("links.pdf"), None).expect("spawn");

    let err = handle
        .crop_page(99, Some((10.0, 10.0, 100.0, 100.0)))
        .await
        .expect_err("out-of-range crop must error");
    assert!(format!("{err}").contains("out of range"), "got: {err}");

    let state = handle.history_state().await.expect("history");
    assert!(!state.can_undo, "a failed crop must record no undo entry");
}

#[tokio::test]
async fn crop_rejects_inverted_rect() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("links.pdf"), None).expect("spawn");

    // left >= right
    let err = handle
        .crop_page(0, Some((500.0, 50.0, 100.0, 742.0)))
        .await
        .expect_err("inverted rect must error");
    assert!(format!("{err}").contains("invalid crop rect"), "got: {err}");
}

/// Writes a cropped doc to `/tmp/vibepdf-verify-cropped.pdf` for the manual
/// cross-reader check. Ignored — run on demand:
///   cargo test --test crop crop_writes_verification_artifact -- --ignored
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn crop_writes_verification_artifact() {
    let out = PathBuf::from("/tmp/vibepdf-verify-cropped.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("links.pdf"), None).expect("spawn");

    // Inset 100pt on every edge of page 0 so the crop is obvious.
    handle.crop_page(0, Some((100.0, 100.0, 512.0, 692.0))).await.expect("crop");
    handle.save(Some(out.clone())).await.expect("save-as");

    assert!(out.is_file(), "artifact should exist at {}", out.display());
    eprintln!("wrote cropped verification artifact to {}", out.display());
}
