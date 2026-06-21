//! Integration tests for stamp annotations (P3.C3a).
//!
//! SPEC: P3-ANN-006 — place a `/Stamp` with a generated `/AP`, persisted through
//! the PDFium save round-trip and undoable. Exercised through the actor against
//! `hello.pdf`.

use std::path::{Path, PathBuf};

use lopdf::{Document, Object};
use vibepdf_lib::pdf::actor::DocumentActorHandle;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
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
    handle.save(Some(out.clone())).await.expect("save");
    eprintln!("wrote stamp verification artifact to {}", out.display());

    drop(handle);
}
