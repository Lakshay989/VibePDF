//! Integration tests for text-markup annotations (P3.B1b).
//!
//! SPEC: P3-ANN-001 — add a standard PDF text-markup annotation over selected
//! text; it must persist through the PDFium save round-trip (with its `/AP`
//! appearance) and be undoable. Exercised through the actor against `hello.pdf`.

use std::path::{Path, PathBuf};

use lopdf::{Dictionary, Document, Object};
use vibepdf_lib::pdf::actor::DocumentActorHandle;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn temp_subdir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("vibepdf-markup-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

fn quad() -> Vec<[f32; 8]> {
    vec![[72.0, 700.0, 200.0, 700.0, 72.0, 688.0, 200.0, 688.0]]
}

/// The annotation dicts on a 0-based page of a saved PDF, read via lopdf
/// (handles `/Annots` as an inline or indirect array).
fn saved_annots(path: &Path, page_index: usize) -> Vec<Dictionary> {
    let bytes = std::fs::read(path).expect("read");
    let doc = Document::load_mem(&bytes).expect("load");
    let Some(&page_id) = doc.get_pages().get(&(page_index as u32 + 1)) else {
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

#[tokio::test]
async fn markup_highlight_persists_through_save_with_ap() {
    let dir = temp_subdir();
    let out = dir.join("hl.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    let state = handle
        .add_text_markup(0, "highlight".into(), quad(), "#ffd400".into(), 1.0)
        .await
        .expect("markup");
    assert!(state.can_undo, "a markup must be undoable");

    handle.save(Some(out.clone())).await.expect("save");
    let annots = saved_annots(&out, 0);
    assert_eq!(annots.len(), 1, "one annotation on page 0");
    assert_eq!(annots[0].get(b"Subtype").unwrap().as_name().unwrap(), b"Highlight");
    assert!(annots[0].get(b"AP").is_ok(), "the /AP appearance survives the PDFium round-trip");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn markup_undo_removes_it() {
    let dir = temp_subdir();
    let out = dir.join("undo.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    handle.add_text_markup(0, "underline".into(), quad(), "#ff0000".into(), 1.0).await.expect("markup");
    let after_undo = handle.undo().await.expect("undo");
    assert!(after_undo.can_redo);

    handle.save(Some(out.clone())).await.expect("save");
    assert_eq!(saved_annots(&out, 0).len(), 0, "undo removes the annotation");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn markup_rejects_empty_quads() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    let err = handle
        .add_text_markup(0, "highlight".into(), vec![], "#ffd400".into(), 1.0)
        .await
        .expect_err("empty quads must error");
    assert!(format!("{err}").contains("no quads"), "got: {err}");

    let state = handle.history_state().await.expect("history");
    assert!(!state.can_undo, "a failed markup records no undo entry");
}

/// Writes a highlighted doc to `/tmp/vibepdf-verify-highlight.pdf` for the
/// manual cross-reader check. Ignored — run on demand:
///   cargo test --test text_markup markup_writes_verification_artifact -- --ignored
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn markup_writes_verification_artifact() {
    let out = PathBuf::from("/tmp/vibepdf-verify-highlight.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    // A highlight roughly over the "Hello, Vibe.PDF." line near the page top.
    let quads = vec![[72.0, 715.0, 320.0, 715.0, 72.0, 700.0, 320.0, 700.0]];
    handle.add_text_markup(0, "highlight".into(), quads, "#ffd400".into(), 1.0).await.expect("markup");
    handle.save(Some(out.clone())).await.expect("save");

    assert!(out.is_file(), "artifact should exist at {}", out.display());
    eprintln!("wrote highlight verification artifact to {}", out.display());
}
