//! Integration tests for polygon + polyline annotations (P3.C1b₂).
//!
//! SPEC: P3-ANN-004 — place a `/Polygon` / `/PolyLine` with a generated `/AP`,
//! persisted through the PDFium save round-trip and undoable. Exercised through
//! the actor against `hello.pdf`.

use std::path::{Path, PathBuf};

use lopdf::{Document, Object};
use vibepdf_lib::pdf::actor::DocumentActorHandle;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn temp_subdir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("vibepdf-poly-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

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
            d.get(b"Subtype").and_then(Object::as_name).ok().is_some_and(|s| wanted.contains(&s))
        })
        .count()
}

#[tokio::test]
async fn polygon_and_polyline_persist_through_save() {
    let dir = temp_subdir();
    let out = dir.join("poly.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    let state = handle
        .add_polygon(
            0,
            true,
            vec![[100.0, 700.0], [200.0, 700.0], [150.0, 620.0]],
            "#ff0000".into(),
            Some("#ffeeee".into()),
            1.0,
            2.0,
        )
        .await
        .expect("polygon");
    assert!(state.can_undo, "a polygon must be undoable");
    handle
        .add_polygon(0, false, vec![[60.0, 400.0], [160.0, 440.0], [260.0, 400.0]], "#0000ff".into(), None, 0.9, 3.0)
        .await
        .expect("polyline");

    handle.save(Some(out.clone())).await.expect("save");
    assert_eq!(count_subtypes(&out, &[b"Polygon"]), 1, "the /Polygon survives");
    assert_eq!(count_subtypes(&out, &[b"PolyLine"]), 1, "the /PolyLine survives");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn polygon_undo_removes_it() {
    let dir = temp_subdir();
    let out = dir.join("undo.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    handle
        .add_polygon(0, true, vec![[50.0, 50.0], [250.0, 50.0], [150.0, 200.0]], "#000000".into(), None, 1.0, 2.0)
        .await
        .expect("add");
    let after_undo = handle.undo().await.expect("undo");
    assert!(after_undo.can_redo, "undo of add-polygon enables redo");

    handle.save(Some(out.clone())).await.expect("save");
    assert_eq!(count_subtypes(&out, &[b"Polygon", b"PolyLine"]), 0, "undo removed the polygon");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn polygon_rejects_too_few_points() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");
    let err = handle
        .add_polygon(0, true, vec![[0.0, 0.0], [10.0, 10.0]], "#000000".into(), None, 1.0, 2.0)
        .await;
    assert!(err.is_err(), "a polygon needs at least 3 points");
    drop(handle);
}

/// Writes a polygon PDF to the git-ignored `Sample PDFs/` for the manual
/// cross-reader ritual. Ignored; run on demand:
///   cargo test --test polygons polygon_writes_verification_artifact -- --ignored
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn polygon_writes_verification_artifact() {
    let out = PathBuf::from("../Sample PDFs/vibepdf-verify-polygon.pdf");
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).expect("ensure Sample PDFs dir");
    }
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");
    handle
        .add_polygon(
            0,
            true,
            vec![[100.0, 700.0], [300.0, 710.0], [320.0, 600.0], [180.0, 560.0], [90.0, 630.0]],
            "#c0392b".into(),
            Some("#f9e2e0".into()),
            1.0,
            2.0,
        )
        .await
        .expect("polygon");
    handle.save(Some(out.clone())).await.expect("save");
    eprintln!("wrote polygon verification artifact to {}", out.display());

    drop(handle);
}
