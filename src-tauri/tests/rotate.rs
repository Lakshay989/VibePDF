//! Integration tests for page rotation (P2.B1).
//!
//! SPEC: P2-PAGE-001 — rotation persists as PDFium `/Rotate`, not a
//! viewer transform. These rotate via the actor (so they exercise the A3
//! undo stack and A1 dirty flag end to end — this is the first real
//! edit), save to a temp file, reopen, and assert the rotation survived.

use std::path::{Path, PathBuf};

use pdfium_render::prelude::PdfPageRenderRotation;
use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::document::open_pdf;

fn hello_pdf() -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic/hello.pdf");
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn temp_subdir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("vibepdf-rotate-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

/// Open `path` independently and read the persisted `/Rotate` of `page`.
fn rotation_of(path: &Path, page: i32) -> PdfPageRenderRotation {
    let (doc, _m) = open_pdf(path, None).expect("open for rotation check");
    let r = doc
        .pages()
        .get(page)
        .expect("get page")
        .rotation()
        .expect("read rotation");
    drop(doc);
    r
}

#[tokio::test]
async fn rotate_persists_through_save_and_reopen() {
    // SPEC: P2-PAGE-001 — the acceptance line: rotate → save → reopen,
    // rotation is on the page dict.
    let dir = temp_subdir();
    let out = dir.join("rotated.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, hello_pdf(), None).expect("spawn");

    let state = handle.rotate_pages(vec![0], 1).await.expect("rotate 90");
    assert!(state.can_undo, "a rotation must be undoable");

    handle.save(Some(out.clone())).await.expect("save-as");
    assert_eq!(rotation_of(&out, 0), PdfPageRenderRotation::Degrees90);

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn rotate_then_undo_restores_original() {
    let dir = temp_subdir();
    let out = dir.join("undone.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, hello_pdf(), None).expect("spawn");

    handle.rotate_pages(vec![0], 2).await.expect("rotate 180");
    let after_undo = handle.undo().await.expect("undo");
    assert!(!after_undo.can_undo, "stack empty after undoing the only edit");
    assert!(after_undo.can_redo, "redo available after undo");

    handle.save(Some(out.clone())).await.expect("save-as");
    assert_eq!(
        rotation_of(&out, 0),
        PdfPageRenderRotation::None,
        "undo must restore the original rotation"
    );

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn redo_reapplies_rotation() {
    let dir = temp_subdir();
    let out = dir.join("redone.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, hello_pdf(), None).expect("spawn");

    handle.rotate_pages(vec![0], 1).await.expect("rotate 90");
    handle.undo().await.expect("undo");
    let after_redo = handle.redo().await.expect("redo");
    assert!(after_redo.can_undo);
    assert!(!after_redo.can_redo);

    handle.save(Some(out.clone())).await.expect("save-as");
    assert_eq!(rotation_of(&out, 0), PdfPageRenderRotation::Degrees90);

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn rotate_out_of_range_is_a_typed_error_and_records_nothing() {
    // Atomicity: a bad index fails before mutating, leaving the undo
    // stack empty (no half-applied edit).
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, hello_pdf(), None).expect("spawn");

    let err = handle
        .rotate_pages(vec![99], 1)
        .await
        .expect_err("out-of-range index must error");
    assert!(
        format!("{err}").contains("out of range"),
        "unexpected error: {err}"
    );

    let state = handle.history_state().await.expect("history_state");
    assert!(
        !state.can_undo,
        "a failed rotation must not record an undo entry"
    );
}

/// Writes a rotated, saved PDF to `/tmp/vibepdf-verify-rotated.pdf` for
/// the manual cross-reader check. Ignored by default — run on demand:
///   cargo test --test rotate rotate_writes_verification_artifact -- --ignored
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn rotate_writes_verification_artifact() {
    let out = PathBuf::from("/tmp/vibepdf-verify-rotated.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, hello_pdf(), None).expect("spawn");

    handle.rotate_pages(vec![0], 1).await.expect("rotate 90");
    handle.save(Some(out.clone())).await.expect("save-as");

    assert!(out.is_file(), "artifact should exist at {}", out.display());
    assert_eq!(rotation_of(&out, 0), PdfPageRenderRotation::Degrees90);
    eprintln!("wrote rotated verification artifact to {}", out.display());
}
