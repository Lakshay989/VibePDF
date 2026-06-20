//! Integration tests for freehand ink annotations (P3.C2).
//!
//! SPEC: P3-ANN-005 — place an `/Ink` annotation with a generated variable-width
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
    let d = std::env::temp_dir().join(format!("vibepdf-ink-test-{}", uuid::Uuid::new_v4()));
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

fn stroke() -> Vec<[f32; 3]> {
    vec![
        [100.0, 700.0, 0.4],
        [140.0, 680.0, 0.7],
        [180.0, 700.0, 1.0],
        [220.0, 670.0, 0.6],
        [260.0, 700.0, 0.3],
    ]
}

#[tokio::test]
async fn ink_persists_through_save() {
    let dir = temp_subdir();
    let out = dir.join("ink.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    let state = handle.add_ink(0, stroke(), "#1f6feb".into(), 1.0, 2.5).await.expect("ink");
    assert!(state.can_undo, "an ink stroke must be undoable");

    handle.save(Some(out.clone())).await.expect("save");
    assert_eq!(count_subtype(&out, b"Ink"), 1, "the /Ink survives the PDFium round-trip");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn ink_undo_removes_it() {
    let dir = temp_subdir();
    let out = dir.join("undo.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    handle.add_ink(0, stroke(), "#000000".into(), 1.0, 2.0).await.expect("add");
    let after_undo = handle.undo().await.expect("undo");
    assert!(after_undo.can_redo, "undo of add-ink enables redo");

    handle.save(Some(out.clone())).await.expect("save");
    assert_eq!(count_subtype(&out, b"Ink"), 0, "undo removed the ink");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn ink_rejects_a_tap() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");
    let err = handle.add_ink(0, vec![[50.0, 50.0, 0.5]], "#000000".into(), 1.0, 2.0).await;
    assert!(err.is_err(), "a single point is a tap, not a stroke");
    drop(handle);
}

/// Writes an ink PDF to the git-ignored `Sample PDFs/` for the manual
/// cross-reader ritual. Ignored; run on demand:
///   cargo test --test ink ink_writes_verification_artifact -- --ignored
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn ink_writes_verification_artifact() {
    let out = PathBuf::from("../Sample PDFs/vibepdf-verify-ink.pdf");
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).expect("ensure Sample PDFs dir");
    }
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    // A pressure-modulated wave so the variable-width ribbon is visible.
    let mut wave: Vec<[f32; 3]> = Vec::new();
    for i in 0..=40u8 {
        let t = f32::from(i) / 40.0;
        let x = 80.0 + t * 440.0;
        let y = 640.0 + (t * std::f32::consts::PI * 3.0).sin() * 40.0;
        let pressure = 0.2 + 0.8 * (t * std::f32::consts::PI).sin();
        wave.push([x, y, pressure]);
    }
    handle.add_ink(0, wave, "#6f42c1".into(), 1.0, 3.0).await.expect("ink");
    handle.save(Some(out.clone())).await.expect("save");
    eprintln!("wrote ink verification artifact to {}", out.display());

    drop(handle);
}
