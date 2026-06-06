//! Integration tests for insert blank page (P2.B3).
//!
//! SPEC: P2-PAGE-004 — insert a blank page that inherits the adjacent
//! page's size, unless overridden. The inverse is a delete (undo removes
//! the blank, redo re-inserts it). Exercised through the actor against
//! `links.pdf` (3 US-Letter pages).

use std::path::{Path, PathBuf};

use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::document::open_pdf;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn temp_subdir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("vibepdf-insert-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

fn page_count(path: &Path) -> u32 {
    let (doc, meta) = open_pdf(path, None).expect("open for count");
    let n = meta.page_count;
    drop(doc);
    n
}

/// (width, height) in points of `page` (0-based) in the file at `path`.
fn page_dims(path: &Path, page: i32) -> (f32, f32) {
    let (doc, _m) = open_pdf(path, None).expect("open for dims");
    let dims = {
        let p = doc.pages().get(page).expect("get page");
        (p.width().value, p.height().value)
    };
    drop(doc);
    dims
}

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 1.0
}

#[tokio::test]
async fn insert_increases_count_and_persists() {
    let dir = temp_subdir();
    let out = dir.join("inserted.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("links.pdf"), None).expect("spawn");

    let state = handle
        .insert_blank_page(1, None)
        .await
        .expect("insert after page 0");
    assert!(state.can_undo, "an insert must be undoable");

    handle.save(Some(out.clone())).await.expect("save-as");
    assert_eq!(page_count(&out), 4, "one page should have been added");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn blank_inherits_adjacent_dimensions() {
    // SPEC: P2-PAGE-004 — inherit size + orientation from the neighbour.
    let dir = temp_subdir();
    let out = dir.join("inherited.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("links.pdf"), None).expect("spawn");

    handle.insert_blank_page(1, None).await.expect("insert");
    handle.save(Some(out.clone())).await.expect("save-as");

    // links.pdf pages are US Letter (612 × 792); the inserted page at index
    // 1 inherits that from page 0.
    let (w, h) = page_dims(&out, 1);
    assert!(approx(w, 612.0) && approx(h, 792.0), "got {w}×{h}, want 612×792");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn undo_removes_redo_reinserts() {
    let dir = temp_subdir();
    let out = dir.join("redo.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("links.pdf"), None).expect("spawn");

    handle.insert_blank_page(1, None).await.expect("insert");
    let after_undo = handle.undo().await.expect("undo");
    assert!(!after_undo.can_undo);
    assert!(after_undo.can_redo);

    handle.save(Some(out.clone())).await.expect("save-as");
    assert_eq!(page_count(&out), 3, "undo must remove the blank page");

    handle.redo().await.expect("redo");
    let out2 = dir.join("redo2.pdf");
    handle.save(Some(out2.clone())).await.expect("save-as");
    assert_eq!(page_count(&out2), 4, "redo must re-insert the blank page");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn prepend_append_and_out_of_range() {
    // `page_count()` is the *cached* count from open; after edits use
    // `metadata_live()`, which re-reads the document.
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("links.pdf"), None).expect("spawn");

    // Prepend (index 0) is valid.
    handle.insert_blank_page(0, None).await.expect("prepend");
    let count = handle.metadata_live().await.expect("meta").page_count;
    assert_eq!(count, 4);

    // Append (index == live page count) is valid.
    handle
        .insert_blank_page(i32::try_from(count).unwrap(), None)
        .await
        .expect("append");
    assert_eq!(handle.metadata_live().await.expect("meta").page_count, 5);

    // Beyond the end is an atomic typed error that adds nothing.
    let err = handle
        .insert_blank_page(99, None)
        .await
        .expect_err("out-of-range insert must error");
    assert!(format!("{err}").contains("out of range"), "got: {err}");
    assert_eq!(handle.metadata_live().await.expect("meta").page_count, 5);
}

/// Writes a doc with an inserted blank page to `/tmp/vibepdf-verify-inserted.pdf`
/// for the manual cross-reader check. Ignored — run on demand:
///   cargo test --test insert_blank insert_writes_verification_artifact -- --ignored
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn insert_writes_verification_artifact() {
    let out = PathBuf::from("/tmp/vibepdf-verify-inserted.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("links.pdf"), None).expect("spawn");

    handle.insert_blank_page(1, None).await.expect("insert after page 0");
    handle.save(Some(out.clone())).await.expect("save-as");

    assert!(out.is_file(), "artifact should exist at {}", out.display());
    assert_eq!(page_count(&out), 4);
    eprintln!("wrote inserted verification artifact to {}", out.display());
}
