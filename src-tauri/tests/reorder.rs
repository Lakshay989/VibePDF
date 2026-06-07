//! Integration tests for reorder (P2.C1).
//!
//! SPEC: P2-PAGE-002 — reorder pages, keeping references valid. Exercised
//! through the actor against `links.pdf` (3 pages; a `/Link` annotation on
//! page 1 marks that page so we can assert where it moved without extracting
//! text). The reorder runs through the lopdf COS layer and replaces the
//! actor's live document. Single-threaded (PDFium).

use std::path::PathBuf;

use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::document::open_pdf;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

async fn live_count(handle: &DocumentActorHandle) -> u32 {
    handle.metadata_live().await.expect("metadata").page_count
}

/// Save the actor's current document to a temp file, reopen it, and report
/// which 0-based page carries an annotation (the marked page in `links.pdf`).
async fn annotated_page_index(handle: &DocumentActorHandle, n: u32) -> Option<u32> {
    let dir = std::env::temp_dir().join(format!("vibepdf-reorder-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let out = dir.join("snap.pdf");
    handle.save(Some(out.clone())).await.expect("save snapshot");

    let (doc, _meta) = open_pdf(&out, None).expect("open snapshot");
    let found = {
        let pages = doc.pages();
        let mut f = None;
        for i in 0..n {
            let page = pages.get(i32::try_from(i).expect("page index fits i32")).expect("page");
            if !page.annotations().is_empty() {
                f = Some(i);
                break;
            }
        }
        f
    };
    drop(doc);
    let _ = std::fs::remove_dir_all(&dir);
    found
}

#[tokio::test]
async fn reorder_moves_annotated_page_and_is_undoable() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("links.pdf"), None).expect("spawn");

    // Page 0 (the link page) starts at index 0.
    assert_eq!(annotated_page_index(&handle, 3).await, Some(0));

    // Move page 0 to the end → new order [1, 2, 0].
    let hist = handle.reorder_pages(vec![1, 2, 0]).await.expect("reorder");
    assert!(hist.can_undo);
    assert_eq!(live_count(&handle).await, 3, "reorder keeps the page count");
    assert_eq!(annotated_page_index(&handle, 3).await, Some(2), "link page moved to the end");

    // Undo restores the original order.
    handle.undo().await.expect("undo");
    assert_eq!(annotated_page_index(&handle, 3).await, Some(0), "undo restores order");

    // Redo re-applies it.
    handle.redo().await.expect("redo");
    assert_eq!(annotated_page_index(&handle, 3).await, Some(2), "redo re-applies");

    drop(handle);
}

#[tokio::test]
async fn reorder_validates_permutation() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("links.pdf"), None).expect("spawn");

    // Wrong length.
    let e1 = handle.reorder_pages(vec![0, 1]).await.expect_err("wrong length");
    assert!(format!("{e1}").contains("flat page tree") || format!("{e1}").contains("permutation"), "got: {e1}");

    // Not a bijection (duplicate / out of range).
    let e2 = handle.reorder_pages(vec![0, 0, 0]).await.expect_err("not a bijection");
    assert!(format!("{e2}").contains("permutation"), "got: {e2}");

    let e3 = handle.reorder_pages(vec![0, 1, 9]).await.expect_err("out of range");
    assert!(format!("{e3}").contains("permutation"), "got: {e3}");

    // No mutation occurred.
    assert_eq!(live_count(&handle).await, 3);
    assert_eq!(annotated_page_index(&handle, 3).await, Some(0));

    drop(handle);
}

/// Writes a reordered PDF to `/tmp/vibepdf-verify-reordered.pdf` for the manual
/// cross-reader check. Ignored — run on demand:
///   cargo test --test reorder reorder_writes_verification_artifact -- --ignored
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn reorder_writes_verification_artifact() {
    let out = PathBuf::from("/tmp/vibepdf-verify-reordered.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("links.pdf"), None).expect("spawn");

    handle.reorder_pages(vec![2, 0, 1]).await.expect("reorder");
    handle.save(Some(out.clone())).await.expect("save");
    assert!(out.is_file());
    eprintln!("wrote reordered verification artifact to {}", out.display());

    drop(handle);
}
