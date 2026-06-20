//! Integration tests for line + arrow annotations (P3.C1b₁).
//!
//! SPEC: P3-ANN-004 — place a `/Line` (optionally an open-arrow) with a generated
//! `/AP`, persisted through the PDFium save round-trip and undoable. Exercised
//! through the actor against `hello.pdf`.

use std::path::{Path, PathBuf};

use lopdf::{Document, Object};
use vibepdf_lib::pdf::actor::DocumentActorHandle;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn temp_subdir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("vibepdf-line-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

fn line_count(path: &Path) -> usize {
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
        .filter(|d| d.get(b"Subtype").and_then(Object::as_name).ok() == Some(&b"Line"[..]))
        .count()
}

#[tokio::test]
async fn line_and_arrow_persist_through_save() {
    let dir = temp_subdir();
    let out = dir.join("lines.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    let state = handle
        .add_line(0, 100.0, 700.0, 300.0, 650.0, false, "#ff0000".into(), 1.0, 2.0)
        .await
        .expect("line");
    assert!(state.can_undo, "a line must be undoable");
    handle
        .add_line(0, 120.0, 500.0, 360.0, 520.0, true, "#0000ff".into(), 0.9, 3.0)
        .await
        .expect("arrow");

    handle.save(Some(out.clone())).await.expect("save");
    assert_eq!(line_count(&out), 2, "both /Line annotations survive");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn line_undo_removes_it() {
    let dir = temp_subdir();
    let out = dir.join("undo.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    handle.add_line(0, 50.0, 50.0, 250.0, 150.0, false, "#000000".into(), 1.0, 2.0).await.expect("add");
    let after_undo = handle.undo().await.expect("undo");
    assert!(after_undo.can_redo, "undo of add-line enables redo");

    handle.save(Some(out.clone())).await.expect("save");
    assert_eq!(line_count(&out), 0, "undo removed the line");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Writes a line+arrow PDF to the git-ignored `Sample PDFs/` for the manual
/// cross-reader ritual. Ignored; run on demand:
///   cargo test --test lines line_writes_verification_artifact -- --ignored
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn line_writes_verification_artifact() {
    let out = PathBuf::from("../Sample PDFs/vibepdf-verify-lines.pdf");
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).expect("ensure Sample PDFs dir");
    }
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");
    handle.add_line(0, 90.0, 700.0, 320.0, 700.0, false, "#c0392b".into(), 2.0, 2.0).await.expect("line");
    handle.add_line(0, 90.0, 640.0, 320.0, 590.0, true, "#2471a3".into(), 1.0, 3.0).await.expect("arrow");
    handle.save(Some(out.clone())).await.expect("save");
    eprintln!("wrote line verification artifact to {}", out.display());

    drop(handle);
}
