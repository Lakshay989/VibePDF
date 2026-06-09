//! Integration tests for page deletion (P2.B2).
//!
//! SPEC: P2-PAGE-003 — delete one or more pages; renumber; surviving
//! internal references stay correct; deletion is undoable. Exercises the
//! actor's `DeletePages` (so the A3 undo stack + A1 dirty flag run end to
//! end) against `links.pdf`, whose page 1 has an internal link to page 3.

use std::path::{Path, PathBuf};

use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::document::open_pdf;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn temp_subdir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("vibepdf-delete-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

fn page_count(path: &Path) -> u32 {
    let (doc, meta) = open_pdf(path, None).expect("open for count");
    let n = meta.page_count;
    drop(doc);
    n
}

/// The 0-based page index that page 0's first internal link targets, if
/// any. Used to prove a surviving reference tracks its (renumbered) page.
fn page0_link_target(path: &Path) -> Option<i32> {
    let (doc, _m) = open_pdf(path, None).expect("open for link read");
    let target = {
        let pages = doc.pages();
        let page = pages.get(0).expect("page 0");
        page.links()
            .iter()
            .find_map(|link| link.destination().and_then(|d| d.page_index().ok()))
    };
    drop(doc);
    target
}

#[tokio::test]
async fn delete_drops_page_count_and_persists() {
    let dir = temp_subdir();
    let out = dir.join("deleted.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("links.pdf"), None).expect("spawn");

    let state = handle.delete_pages(vec![1]).await.expect("delete page 2");
    assert!(state.can_undo, "a delete must be undoable");

    handle.save(Some(out.clone())).await.expect("save-as");
    assert_eq!(page_count(&out), 2, "one page should be gone");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn undo_restores_count_and_order() {
    let dir = temp_subdir();
    let out = dir.join("restored.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("links.pdf"), None).expect("spawn");

    handle.delete_pages(vec![1]).await.expect("delete page 2");
    let after_undo = handle.undo().await.expect("undo");
    assert!(!after_undo.can_undo);
    assert!(after_undo.can_redo);

    handle.save(Some(out.clone())).await.expect("save-as");
    assert_eq!(page_count(&out), 3, "undo must restore the page count");
    // Order check: page 3 is back at index 2, so the page-1 link (which
    // targets page 3's object) resolves to index 2 again — meaning the
    // removed page was re-inserted at its original middle position.
    assert_eq!(
        page0_link_target(&out),
        Some(2),
        "undo must restore the original page order"
    );

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn redo_redeletes() {
    let dir = temp_subdir();
    let out = dir.join("redeleted.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("links.pdf"), None).expect("spawn");

    handle.delete_pages(vec![1]).await.expect("delete");
    handle.undo().await.expect("undo");
    handle.redo().await.expect("redo");

    handle.save(Some(out.clone())).await.expect("save-as");
    assert_eq!(page_count(&out), 2, "redo must re-apply the delete");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn surviving_link_target_stays_correct() {
    // SPEC: P2-PAGE-003 acceptance — "delete page 3 of a doc with an
    // internal link to page 4 → the link now targets the new page 3."
    // Here: link page1 → page3; delete page 2; the link must follow page 3
    // to its new index 1. Works because PDF destinations are object refs.
    let dir = temp_subdir();
    let out = dir.join("link.pdf");

    // Sanity: the fixture's link targets page 3 (index 2).
    assert_eq!(page0_link_target(&fixture("links.pdf")), Some(2));

    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("links.pdf"), None).expect("spawn");
    handle.delete_pages(vec![1]).await.expect("delete page 2");
    handle.save(Some(out.clone())).await.expect("save-as");

    assert_eq!(
        page0_link_target(&out),
        Some(1),
        "the surviving link must track page 3 to its new index"
    );

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn delete_out_of_range_is_atomic_typed_error() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("links.pdf"), None).expect("spawn");

    let err = handle
        .delete_pages(vec![99])
        .await
        .expect_err("out-of-range index must error");
    assert!(
        format!("{err}").contains("out of range"),
        "unexpected error: {err}"
    );

    // Nothing deleted, nothing recorded.
    assert_eq!(handle.page_count().await.expect("count"), 3);
    let state = handle.history_state().await.expect("history");
    assert!(!state.can_undo, "a failed delete must record no undo entry");
}

/// SPEC: P2-PAGE-003 — references TO a deleted page are pruned on save.
/// `links.pdf` page 1 links to page 3; delete page 3, save, and the (now
/// dangling) link is gone from the saved file.
#[tokio::test]
async fn delete_prunes_dangling_link() {
    let dir = temp_subdir();
    let out = dir.join("out.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("links.pdf"), None).expect("spawn");

    // Delete page index 2 (page 3 — the link target).
    handle.delete_pages(vec![2]).await.expect("delete page 3");
    handle.save(Some(out.clone())).await.expect("save");

    let (doc, _meta) = open_pdf(&out, None).expect("open output");
    let empty = {
        let pages = doc.pages();
        pages.get(0).expect("page 0").annotations().is_empty()
    };
    assert!(empty, "dangling link pruned on save");
    drop(doc);

    let _ = std::fs::remove_dir_all(&dir);
}

/// SPEC: P2-PAGE-003 — a bookmark pointing at a deleted page is pruned on save.
/// `bookmarks.pdf` has 3 top-level bookmarks (pages 1/3/5); delete page 3 and
/// the middle bookmark (now dangling) is gone, leaving 2.
#[tokio::test]
async fn delete_prunes_dangling_bookmark() {
    let dir = temp_subdir();
    let out = dir.join("out.pdf");
    let id = uuid::Uuid::new_v4();
    let handle =
        DocumentActorHandle::spawn(None, id, fixture("bookmarks.pdf"), None).expect("spawn");

    // Delete page index 2 (page 3 — the middle bookmark's target).
    handle.delete_pages(vec![2]).await.expect("delete page 3");
    handle.save(Some(out.clone())).await.expect("save");

    let (doc, _meta) = open_pdf(&out, None).expect("open output");
    let mut count = 0;
    let mut node = doc.bookmarks().root();
    while let Some(b) = node {
        count += 1;
        node = b.next_sibling();
    }
    assert_eq!(count, 2, "dangling bookmark pruned on save");
    drop(doc);

    let _ = std::fs::remove_dir_all(&dir);
}

/// Writes a "dangling refs pruned" artifact: `bookmarks.pdf` with page 3
/// deleted, so the bookmarks panel should show 2 entries (Chapter 2 gone).
/// Ignored — run on demand:
///   cargo test --test delete_page prune_writes_verification_artifact -- --ignored
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn prune_writes_verification_artifact() {
    let out = PathBuf::from("/tmp/vibepdf-verify-pruned.pdf");
    let id = uuid::Uuid::new_v4();
    let handle =
        DocumentActorHandle::spawn(None, id, fixture("bookmarks.pdf"), None).expect("spawn");

    handle.delete_pages(vec![2]).await.expect("delete page 3");
    handle.save(Some(out.clone())).await.expect("save");
    assert!(out.is_file());
    eprintln!("wrote pruned verification artifact to {}", out.display());

    drop(handle);
}

/// Writes a deleted, saved PDF to `/tmp/vibepdf-verify-deleted.pdf` for the
/// manual cross-reader check. Ignored by default — run on demand:
///   cargo test --test delete_page delete_writes_verification_artifact -- --ignored
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn delete_writes_verification_artifact() {
    let out = PathBuf::from("/tmp/vibepdf-verify-deleted.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("links.pdf"), None).expect("spawn");

    handle.delete_pages(vec![1]).await.expect("delete page 2");
    handle.save(Some(out.clone())).await.expect("save-as");

    assert!(out.is_file(), "artifact should exist at {}", out.display());
    assert_eq!(page_count(&out), 2);
    eprintln!("wrote deleted verification artifact to {}", out.display());
}
