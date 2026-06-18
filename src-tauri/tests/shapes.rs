//! Integration tests for shape annotations (P3.C1a).
//!
//! SPEC: P3-ANN-004 — place a `/Square` / `/Circle` shape with a generated
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
    let d = std::env::temp_dir().join(format!("vibepdf-shape-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

/// Count page-1 annotations whose `/Subtype` is in `wanted`.
fn count_subtypes(path: &Path, wanted: &[&[u8]]) -> usize {
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
        .filter(|d| {
            d.get(b"Subtype")
                .and_then(Object::as_name)
                .ok()
                .is_some_and(|s| wanted.contains(&s))
        })
        .count()
}

#[tokio::test]
async fn shape_persists_through_save() {
    let dir = temp_subdir();
    let out = dir.join("shape.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    let state = handle
        .add_shape(0, "rectangle".into(), [100.0, 600.0, 300.0, 700.0], "#ff0000".into(), Some("#ffeeee".into()), 1.0, 2.0)
        .await
        .expect("rectangle");
    assert!(state.can_undo, "a shape must be undoable");
    handle
        .add_shape(0, "ellipse".into(), [120.0, 400.0, 320.0, 520.0], "#0000ff".into(), None, 0.8, 3.0)
        .await
        .expect("ellipse");

    handle.save(Some(out.clone())).await.expect("save");
    assert_eq!(count_subtypes(&out, &[b"Square"]), 1, "the /Square survives");
    assert_eq!(count_subtypes(&out, &[b"Circle"]), 1, "the /Circle survives");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn shape_undo_removes_it() {
    let dir = temp_subdir();
    let out = dir.join("undo.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    handle
        .add_shape(0, "rectangle".into(), [50.0, 50.0, 250.0, 150.0], "#000000".into(), None, 1.0, 2.0)
        .await
        .expect("add");
    let after_undo = handle.undo().await.expect("undo");
    assert!(after_undo.can_redo, "undo of add-shape enables redo");

    handle.save(Some(out.clone())).await.expect("save");
    assert_eq!(count_subtypes(&out, &[b"Square", b"Circle"]), 0, "undo removed the shape");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn shape_rejects_empty_rect() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");
    let err = handle
        .add_shape(0, "rectangle".into(), [10.0, 10.0, 10.0, 40.0], "#000000".into(), None, 1.0, 1.0)
        .await;
    assert!(err.is_err(), "an empty rect is rejected");
    drop(handle);
}

/// Writes a shape-bearing PDF to the git-ignored `Sample PDFs/` for the manual
/// cross-reader ritual. Ignored; run on demand:
///   cargo test --test shapes shape_writes_verification_artifact -- --ignored
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn shape_writes_verification_artifact() {
    let out = PathBuf::from("../Sample PDFs/vibepdf-verify-shapes.pdf");
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).expect("ensure Sample PDFs dir");
    }
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");
    handle
        .add_shape(0, "rectangle".into(), [90.0, 620.0, 300.0, 710.0], "#c0392b".into(), Some("#f9e2e0".into()), 1.0, 2.0)
        .await
        .expect("rectangle");
    handle
        .add_shape(0, "ellipse".into(), [110.0, 470.0, 330.0, 580.0], "#2471a3".into(), Some("#d6eaf8".into()), 0.85, 3.0)
        .await
        .expect("ellipse");
    handle.save(Some(out.clone())).await.expect("save");
    eprintln!("wrote shape verification artifact to {}", out.display());

    drop(handle);
}
