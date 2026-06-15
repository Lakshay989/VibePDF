//! Integration tests for sticky notes (P3.B2).
//!
//! SPEC: P3-ANN-002 — place a `/Text` annotation with author + timestamp + body
//! that is re-openable, editable, and deletable; it must persist through the
//! PDFium save round-trip. Exercised through the actor against `hello.pdf`.

use std::path::{Path, PathBuf};

use lopdf::{Dictionary, Document, Object};
use vibepdf_lib::pdf::actor::DocumentActorHandle;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn temp_subdir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("vibepdf-note-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

fn saved_annots(path: &Path) -> Vec<Dictionary> {
    let bytes = std::fs::read(path).expect("read");
    let doc = Document::load_mem(&bytes).expect("load");
    let Some(&page_id) = doc.get_pages().get(&1) else {
        return Vec::new();
    };
    let annots = doc.get_dictionary(page_id).ok().and_then(|p| p.get(b"Annots").ok().cloned());
    let arr = match annots {
        Some(Object::Array(a)) => a,
        Some(Object::Reference(id)) => {
            doc.get_object(id).and_then(Object::as_array).cloned().unwrap_or_default()
        }
        _ => Vec::new(),
    };
    arr.iter()
        .filter_map(|o| o.as_reference().ok())
        .filter_map(|id| doc.get_dictionary(id).ok().cloned())
        .collect()
}

fn str_field(d: &Dictionary, key: &[u8]) -> String {
    String::from_utf8_lossy(d.get(key).and_then(Object::as_str).unwrap_or(b"")).into_owned()
}

#[tokio::test]
async fn note_persists_with_author_contents_and_nm() {
    let dir = temp_subdir();
    let out = dir.join("note.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    let state = handle
        .add_note("note-1".into(), 0, 100.0, 700.0, "a first note".into(), "Tester".into())
        .await
        .expect("add note");
    assert!(state.can_undo, "a note must be undoable");

    handle.save(Some(out.clone())).await.expect("save");
    let annots = saved_annots(&out);
    let note = annots
        .iter()
        .find(|a| a.get(b"Subtype").and_then(Object::as_name).ok() == Some(&b"Text"[..]))
        .expect("a /Text annotation");
    assert_eq!(str_field(note, b"Contents"), "a first note");
    assert_eq!(str_field(note, b"T"), "Tester");
    assert_eq!(str_field(note, b"NM"), "note-1");
    assert!(note.get(b"M").is_ok(), "modification date present");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn note_update_changes_contents() {
    let dir = temp_subdir();
    let out = dir.join("upd.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    handle.add_note("n".into(), 0, 100.0, 700.0, "before".into(), "T".into()).await.expect("add");
    handle.update_note("n".into(), "after".into()).await.expect("update");

    handle.save(Some(out.clone())).await.expect("save");
    let annots = saved_annots(&out);
    let note = annots.iter().find(|a| str_field(a, b"NM") == "n").expect("note by NM");
    assert_eq!(str_field(note, b"Contents"), "after");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn note_delete_removes_it() {
    let dir = temp_subdir();
    let out = dir.join("del.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    handle.add_note("n".into(), 0, 100.0, 700.0, "doomed".into(), "T".into()).await.expect("add");
    handle.delete_annotation("n".into()).await.expect("delete");

    handle.save(Some(out.clone())).await.expect("save");
    assert!(
        !saved_annots(&out).iter().any(|a| str_field(a, b"NM") == "n"),
        "the note should be gone"
    );

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Writes a note-bearing PDF to the git-ignored `Sample PDFs/` for the manual
/// cross-reader ritual (Acrobat / Preview / Okular). Ignored; run on demand:
///   cargo test --test text_note note_writes_verification_artifact -- --ignored
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn note_writes_verification_artifact() {
    let out = PathBuf::from("../Sample PDFs/vibepdf-verify-note.pdf");
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).expect("ensure Sample PDFs dir");
    }
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");
    handle
        .add_note(
            "verify-note".into(),
            0,
            120.0,
            700.0,
            "VibePDF sticky note — P3.B2a verification.".into(),
            "VibePDF User".into(),
        )
        .await
        .expect("add note");
    handle.save(Some(out.clone())).await.expect("save");
    eprintln!("wrote note verification artifact to {}", out.display());

    drop(handle);
}

#[tokio::test]
async fn note_undo_restores_then_removes() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    handle.add_note("n".into(), 0, 100.0, 700.0, "x".into(), "T".into()).await.expect("add");
    let after_undo = handle.undo().await.expect("undo");
    assert!(after_undo.can_redo, "undo of add-note enables redo");

    drop(handle);
}
