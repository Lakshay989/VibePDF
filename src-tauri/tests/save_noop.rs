//! Integration tests for explicit save (P2.A1).
//!
//! SPEC: P2-SAVE-001 (proposed) / NFR-PERF-004. Covers the actor's
//! `Save` message and the underlying `pdf::document::save_document`
//! write path: save-as round-trips, a clean same-path save is a true
//! no-op, and overwriting rotates a `.bak`.
//!
//! Like the other actor tests, these spawn with `app: None` (no Tauri
//! runtime under `cargo test`) and never write near the fixture — every
//! test that mutates works on a fresh copy in a unique temp dir.

use std::path::PathBuf;

use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::document::{open_pdf, save_document};

fn hello_pdf() -> PathBuf {
    // Test runs with CWD = src-tauri/.
    let p = PathBuf::from("../tests/fixtures/basic/hello.pdf");
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

/// A unique, empty temp directory (no `tempfile` crate; same pattern as
/// `tests/recents.rs`). Caller removes it on the happy path.
fn temp_subdir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("vibepdf-save-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

#[tokio::test]
async fn save_as_roundtrips_page_count() {
    // SPEC: P2-SAVE-001 — a save-as always writes, and the written file
    // must re-open in PDFium with the same page count. This is the
    // write+verify path that proves the pipeline end to end.
    let dir = temp_subdir();
    let out = dir.join("saved.pdf");

    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, hello_pdf(), None).expect("spawn");

    let outcome = handle.save(Some(out.clone())).await.expect("save-as should succeed");
    assert!(!outcome.no_op, "an explicit save-as is never a no-op");
    assert_eq!(outcome.path, out.to_string_lossy().into_owned());
    assert!(outcome.bytes_written > 0, "should have written bytes");

    // Re-open the saved file independently of the actor.
    let (doc, meta) = open_pdf(&out, None).expect("saved file should re-open cleanly");
    assert_eq!(meta.page_count, 1);
    drop(doc);

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn save_same_path_not_dirty_is_true_noop() {
    // SPEC: P2-SAVE-001 — saving a clean document to its own path must
    // not rewrite the file at all: the bytes (hence the hash) stay
    // identical. Nothing flips `dirty` true in P2.A1, so every same-path
    // save is currently a no-op.
    let dir = temp_subdir();
    let orig = dir.join("orig.pdf");
    std::fs::copy(hello_pdf(), &orig).expect("copy fixture into temp");
    let before = std::fs::read(&orig).expect("read original bytes");

    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, orig.clone(), None).expect("spawn");

    let outcome = handle.save(None).await.expect("same-path save should succeed");
    assert!(outcome.no_op, "a clean same-path save must be a no-op");
    assert_eq!(outcome.bytes_written, 0);

    let after = std::fs::read(&orig).expect("read original bytes after");
    assert_eq!(before, after, "a no-op save must leave the file byte-identical");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn save_document_rotates_bak_when_overwriting() {
    // SPEC: P2-SAVE-001 — overwriting the original rotates the previous
    // file to `<name>.bak`. Driven against the `save_document` helper
    // directly (with `make_backup = true`) because no edit op exists yet
    // to make a document dirty through the actor.
    let dir = temp_subdir();
    let orig = dir.join("doc.pdf");
    std::fs::copy(hello_pdf(), &orig).expect("copy fixture into temp");

    let (doc, _meta) = open_pdf(&orig, None).expect("open temp copy");
    let outcome = save_document(&doc, &orig, true, None).expect("save with backup");
    assert!(!outcome.no_op);
    drop(doc);

    let bak = dir.join("doc.pdf.bak");
    assert!(bak.is_file(), "backup should exist at {}", bak.display());
    assert!(orig.is_file(), "original path should still hold the new file");

    // Both the rewritten original and the backup must be valid PDFs.
    let (d1, m1) = open_pdf(&orig, None).expect("rewritten file re-opens");
    assert_eq!(m1.page_count, 1);
    drop(d1);
    let (d2, m2) = open_pdf(&bak, None).expect("backup re-opens");
    assert_eq!(m2.page_count, 1);
    drop(d2);

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn undo_to_saved_state_is_true_noop() {
    // SPEC: P2-SAVE-001 / FABLE_REVIEW §3.11 (P4.HF12), bug (a). Editing
    // then undoing back to the exact saved state must leave the document
    // clean — a same-path save is a true no-op again, not a rewrite.
    // Before the fix, undo unconditionally set `dirty = true`, so this save
    // rewrote the file.
    let dir = temp_subdir();
    let orig = dir.join("orig.pdf");
    std::fs::copy(hello_pdf(), &orig).expect("copy fixture into temp");
    let before = std::fs::read(&orig).expect("read original bytes");

    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, orig.clone(), None).expect("spawn");

    handle.rotate_pages(vec![0], 1).await.expect("rotate 90 dirties the doc");
    let after_undo = handle.undo().await.expect("undo back to the as-opened state");
    assert!(!after_undo.can_undo, "undo stack is empty again");

    let outcome = handle.save(None).await.expect("same-path save");
    assert!(outcome.no_op, "undo-to-saved must make the same-path save a no-op");
    assert_eq!(outcome.bytes_written, 0);

    let after = std::fs::read(&orig).expect("read original bytes after");
    assert_eq!(before, after, "a no-op save must leave the file byte-identical");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn save_as_then_same_path_save_is_noop() {
    // SPEC: P2-SAVE-001 / FABLE_REVIEW §3.11 (P4.HF12), bug (b). A save-as
    // persists the in-memory doc, so it becomes clean regardless of path —
    // the "unsaved changes" claim must clear. Before the fix, `dirty` only
    // cleared on a *same-path* save, so a save-as left the identical doc
    // marked dirty (→ stale recovery copy / false prompt).
    //
    // Observable via the same-path no-op: after edit + save-as to B, a
    // same-path save to the original A is a no-op. NOTE (scoped quirk): the
    // actor does not repoint its `path` on save-as, so the rotation lives
    // in B and A stays the original — repointing is out of scope for §3.11.
    let dir = temp_subdir();
    let orig = dir.join("orig.pdf"); // path A
    let other = dir.join("copy.pdf"); // path B (save-as target)
    std::fs::copy(hello_pdf(), &orig).expect("copy fixture into temp");
    let before_a = std::fs::read(&orig).expect("read A bytes");

    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, orig.clone(), None).expect("spawn");

    handle.rotate_pages(vec![0], 1).await.expect("rotate dirties the doc");
    let as_outcome = handle.save(Some(other.clone())).await.expect("save-as to B");
    assert!(!as_outcome.no_op, "a save-as always writes");
    assert!(other.is_file(), "B should exist after save-as");

    // The save-as cleaned the doc, so a same-path save to A no-ops.
    let a_outcome = handle.save(None).await.expect("same-path save to A");
    assert!(a_outcome.no_op, "save-as must clear dirty → same-path save is a no-op");
    let after_a = std::fs::read(&orig).expect("read A bytes after");
    assert_eq!(before_a, after_a, "A stays byte-identical (rotation lives in B)");

    // B is a valid PDF and carries the edit's page count.
    let (doc, meta) = open_pdf(&other, None).expect("B re-opens cleanly");
    assert_eq!(meta.page_count, 1);
    drop(doc);

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Writes a real saved PDF to `/tmp/vibepdf-verify.pdf` for the manual
/// cross-reader verification ritual (Acrobat / Preview / a third reader).
/// Ignored by default — produces an artifact, so run on demand:
///   cargo test --test save_noop save_writes_verification_artifact -- --ignored
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn save_writes_verification_artifact() {
    let out = PathBuf::from("/tmp/vibepdf-verify.pdf");

    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, hello_pdf(), None).expect("spawn");
    let outcome = handle
        .save(Some(out.clone()))
        .await
        .expect("save-as for verification artifact");

    assert!(out.is_file(), "artifact should exist at {}", out.display());
    eprintln!(
        "wrote verification artifact to {} ({} bytes)",
        out.display(),
        outcome.bytes_written
    );
}
