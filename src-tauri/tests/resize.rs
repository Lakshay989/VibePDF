//! Integration tests for resize (P2.B5).
//!
//! SPEC: P2-PAGE-010 — resize one or more pages to a standard or custom size,
//! scaling content to fit (with a preserve-aspect option), undoable. Exercised
//! through the actor against `hello.pdf` / `links.pdf` (US-Letter, 612×792).

use std::path::{Path, PathBuf};

use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::document::open_pdf;

/// A4 in points (1/72").
const A4: (f32, f32) = (595.28, 841.89);

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn temp_subdir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("vibepdf-resize-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

/// (left, bottom, right, top) of `page`'s MediaBox, in points.
fn media_box(path: &Path, page: i32) -> (f32, f32, f32, f32) {
    let (doc, _m) = open_pdf(path, None).expect("open for media read");
    let r = {
        let p = doc.pages().get(page).expect("get page");
        let b = p.boundaries().media().expect("media box").bounds;
        (b.left().value, b.bottom().value, b.right().value, b.top().value)
    };
    drop(doc);
    r
}

fn approx_wh(got: (f32, f32, f32, f32), w: f32, h: f32) -> bool {
    (got.0 - 0.0).abs() < 1.0
        && (got.1 - 0.0).abs() < 1.0
        && (got.2 - w).abs() < 1.0
        && (got.3 - h).abs() < 1.0
}

#[tokio::test]
async fn resize_to_a4_sets_mediabox() {
    let dir = temp_subdir();
    let out = dir.join("a4.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    let state = handle.resize_pages(vec![0], A4.0, A4.1, true).await.expect("resize");
    assert!(state.can_undo, "a resize must be undoable");

    handle.save(Some(out.clone())).await.expect("save-as");
    let got = media_box(&out, 0);
    assert!(approx_wh(got, A4.0, A4.1), "got {got:?}, want A4 {A4:?}");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn resize_all_pages_to_a4() {
    let dir = temp_subdir();
    let out = dir.join("all-a4.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("links.pdf"), None).expect("spawn");

    handle.resize_pages(vec![0, 1, 2], A4.0, A4.1, true).await.expect("resize all");
    handle.save(Some(out.clone())).await.expect("save-as");

    for page in 0..3 {
        let got = media_box(&out, page);
        assert!(approx_wh(got, A4.0, A4.1), "page {page}: got {got:?}, want A4");
    }

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

// NOTE: an automated "content actually scaled, not just relabeled" assertion
// would iterate page objects (`page.objects().iter()` + `obj.bounds()`), but
// that path crashes PDFium at process teardown in pdfium-render 0.9.1 (a known
// object-iteration drop quirk, unrelated to the resize itself). Content-scaling
// fidelity is verified instead via the cross-reader artifact below — the same
// way crop/extract visual correctness is checked.

#[tokio::test]
async fn resize_undo_restores_original_size() {
    let dir = temp_subdir();
    let out = dir.join("undo.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    handle.resize_pages(vec![0], A4.0, A4.1, true).await.expect("resize");
    let after_undo = handle.undo().await.expect("undo");
    assert!(after_undo.can_redo, "undo should enable redo");

    handle.save(Some(out.clone())).await.expect("save-as");
    let got = media_box(&out, 0);
    assert!(
        approx_wh(got, 612.0, 792.0),
        "undo should restore the original Letter size, got {got:?}"
    );

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn resize_rejects_nonpositive_dims() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    let err = handle
        .resize_pages(vec![0], 0.0, 800.0, true)
        .await
        .expect_err("zero width must error");
    assert!(format!("{err}").contains("positive"), "got: {err}");

    let state = handle.history_state().await.expect("history");
    assert!(!state.can_undo, "a failed resize must record no undo entry");
}

#[tokio::test]
async fn resize_out_of_range_is_typed_error() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    let err = handle
        .resize_pages(vec![99], A4.0, A4.1, true)
        .await
        .expect_err("out-of-range page must error");
    assert!(format!("{err}").contains("out of range"), "got: {err}");
}

/// Writes a resized doc to `/tmp/vibepdf-verify-resized.pdf` for the manual
/// cross-reader check. Ignored — run on demand:
///   cargo test --test resize resize_writes_verification_artifact -- --ignored
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn resize_writes_verification_artifact() {
    let out = PathBuf::from("/tmp/vibepdf-verify-resized.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    // Letter → A4, preserve aspect, so the text is visibly scaled to fit.
    handle.resize_pages(vec![0], A4.0, A4.1, true).await.expect("resize");
    handle.save(Some(out.clone())).await.expect("save-as");

    assert!(out.is_file(), "artifact should exist at {}", out.display());
    eprintln!("wrote resized verification artifact to {}", out.display());
}
