//! Integration tests for free-text annotations (P3.B3a).
//!
//! SPEC: P3-ANN-003 — place a `/FreeText` box (uniform style) with a generated
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
    let d = std::env::temp_dir().join(format!("vibepdf-freetext-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

/// Count the `/FreeText` annotations on page 1 of the saved file.
fn free_text_count(path: &Path) -> usize {
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
        .filter(|d| d.get(b"Subtype").and_then(Object::as_name).ok() == Some(&b"FreeText"[..]))
        .count()
}

#[tokio::test]
async fn free_text_persists_through_save() {
    let dir = temp_subdir();
    let out = dir.join("ft.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    let state = handle
        .add_free_text(
            0,
            [100.0, 600.0, 320.0, 700.0],
            "Hello\nWorld".into(),
            "Times".into(),
            18.0,
            "#1133ff".into(),
            true,
            false,
        )
        .await
        .expect("add free text");
    assert!(state.can_undo, "a free-text box must be undoable");

    handle.save(Some(out.clone())).await.expect("save");
    assert_eq!(free_text_count(&out), 1, "the /FreeText survives the PDFium save");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn free_text_undo_removes_it() {
    let dir = temp_subdir();
    let out = dir.join("undo.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    handle
        .add_free_text(0, [50.0, 50.0, 250.0, 90.0], "x".into(), "Helvetica".into(), 12.0, "#000000".into(), false, false)
        .await
        .expect("add");
    let after_undo = handle.undo().await.expect("undo");
    assert!(after_undo.can_redo, "undo of add-free-text enables redo");

    handle.save(Some(out.clone())).await.expect("save");
    assert_eq!(free_text_count(&out), 0, "undo removed the box before save");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn free_text_rejects_empty_rect() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");
    let err = handle
        .add_free_text(0, [10.0, 10.0, 10.0, 40.0], "x".into(), "Helvetica".into(), 12.0, "#000000".into(), false, false)
        .await;
    assert!(err.is_err(), "an empty rect is rejected");
    drop(handle);
}

/// Writes a free-text-bearing PDF to the git-ignored `Sample PDFs/` for the
/// manual cross-reader ritual. Ignored; run on demand:
///   cargo test --test free_text free_text_writes_verification_artifact -- --ignored
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn free_text_writes_verification_artifact() {
    let out = PathBuf::from("../Sample PDFs/vibepdf-verify-freetext.pdf");
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).expect("ensure Sample PDFs dir");
    }
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");
    handle
        .add_free_text(
            0,
            [100.0, 600.0, 360.0, 700.0],
            "VibePDF free text — P3.B3a.\nLine two, Times Bold 18.".into(),
            "Times".into(),
            18.0,
            "#cc1111".into(),
            true,
            false,
        )
        .await
        .expect("add free text");
    handle.save(Some(out.clone())).await.expect("save");
    eprintln!("wrote free-text verification artifact to {}", out.display());

    drop(handle);
}
