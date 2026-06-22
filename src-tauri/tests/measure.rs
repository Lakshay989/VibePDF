//! Integration tests for measurement annotations (P3.C4a).
//!
//! SPEC: P3-ANN-007 — place a distance / perimeter / area measurement (a
//! `/Line` / `/PolyLine` / `/Polygon` with a dimension `/IT` + a value label),
//! persisted through the PDFium save round-trip and undoable. Through the actor
//! against `hello.pdf`.

use std::path::{Path, PathBuf};

use lopdf::{Document, Object};
use vibepdf_lib::pdf::actor::DocumentActorHandle;

/// Read the `/Measure` dict of the first measurement annotation on page 1.
fn first_measure_dict(path: &Path) -> Option<lopdf::Dictionary> {
    let bytes = std::fs::read(path).expect("read");
    let doc = Document::load_mem(&bytes).expect("load");
    let page_id = *doc.get_pages().get(&1)?;
    let arr = match doc.get_dictionary(page_id).ok().and_then(|p| p.get(b"Annots").ok().cloned()) {
        Some(Object::Array(a)) => a,
        Some(Object::Reference(id)) => {
            doc.get_object(id).and_then(Object::as_array).cloned().unwrap_or_default()
        }
        _ => return None,
    };
    arr.iter()
        .filter_map(|o| o.as_reference().ok())
        .filter_map(|id| doc.get_dictionary(id).ok())
        .find_map(|d| d.get(b"Measure").and_then(Object::as_dict).cloned().ok())
}

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn temp_subdir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("vibepdf-measure-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

fn count_with_intent(path: &Path) -> usize {
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
        .filter(|d| d.get(b"IT").and_then(Object::as_name).ok().is_some_and(|it| it.ends_with(b"Dimension")))
        .count()
}

#[tokio::test]
async fn measure_persists_through_save() {
    let dir = temp_subdir();
    let out = dir.join("measure.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    let state = handle
        .add_measure(0, "distance".into(), vec![[100.0, 700.0], [300.0, 700.0]], "#1f6feb".into(), "4 m".into(), 1.0, 1.5, 0.02, "m".into())
        .await
        .expect("distance");
    assert!(state.can_undo, "a measurement must be undoable");
    handle
        .add_measure(0, "area".into(), vec![[100.0, 600.0], [220.0, 600.0], [160.0, 520.0]], "#1e8449".into(), "0.5 m²".into(), 1.0, 1.5, 0.02, "m".into())
        .await
        .expect("area");

    handle.save(Some(out.clone())).await.expect("save");
    assert_eq!(count_with_intent(&out), 2, "both measurements survive the round-trip");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn measure_undo_removes_it() {
    let dir = temp_subdir();
    let out = dir.join("undo.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    handle
        .add_measure(0, "distance".into(), vec![[100.0, 700.0], [300.0, 700.0]], "#000000".into(), "4 m".into(), 1.0, 1.5, 1.0, "pt".into())
        .await
        .expect("add");
    let after_undo = handle.undo().await.expect("undo");
    assert!(after_undo.can_redo, "undo of add-measure enables redo");

    handle.save(Some(out.clone())).await.expect("save");
    assert_eq!(count_with_intent(&out), 0, "undo removed the measurement");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

// SPEC: P3-ANN-007 (P3.C4b)
#[tokio::test]
async fn measure_writes_measure_dict() {
    let dir = temp_subdir();
    let out = dir.join("measure-dict.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    handle
        .add_measure(0, "distance".into(), vec![[100.0, 700.0], [300.0, 700.0]], "#1f6feb".into(), "4 m".into(), 1.0, 1.5, 0.02, "m".into())
        .await
        .expect("distance");
    handle.save(Some(out.clone())).await.expect("save");

    let measure = first_measure_dict(&out).expect("a /Measure dict is attached");
    assert_eq!(measure.get(b"Subtype").and_then(Object::as_name).ok(), Some(&b"RL"[..]), "rectilinear");
    let x0 = measure.get(b"X").and_then(Object::as_array).expect("/X").first().expect("/X[0]");
    let x0 = x0.as_dict().expect("number format");
    let c = match x0.get(b"C").expect("/C") {
        Object::Real(r) => *r,
        other => panic!("/C should be a real, got {other:?}"),
    };
    assert!((c - 0.02).abs() < 1e-6, "/X /C carries the scale (units per point): {c}");
    assert_eq!(x0.get(b"U").and_then(Object::as_str).ok(), Some(&b"m"[..]), "unit label");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

// SPEC: P3-ANN-007 (P3.C4b) — calibration round-trips through the /Measure dict.
#[tokio::test]
async fn measure_calibration_round_trips() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    // Uncalibrated docs report nothing.
    assert!(handle.read_measure_calibration().await.expect("read").is_none());

    handle
        .add_measure(0, "distance".into(), vec![[100.0, 700.0], [300.0, 700.0]], "#000000".into(), "100 ft".into(), 1.0, 1.5, 0.5, "ft".into())
        .await
        .expect("measure");

    let cal = handle.read_measure_calibration().await.expect("read").expect("a calibration");
    assert!((cal.units_per_point - 0.5).abs() < 1e-6, "scale persisted: {}", cal.units_per_point);
    assert_eq!(cal.unit, "ft", "unit persisted");

    drop(handle);
}

#[tokio::test]
async fn measure_rejects_bad_kind() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");
    let err = handle
        .add_measure(0, "volume".into(), vec![[0.0, 0.0], [10.0, 10.0]], "#000000".into(), "x".into(), 1.0, 1.0, 1.0, "pt".into())
        .await;
    assert!(err.is_err(), "an unknown measure kind is rejected");
    drop(handle);
}

/// Writes a measurement PDF to the git-ignored `Sample PDFs/` for the manual
/// cross-reader ritual. Ignored; run on demand:
///   cargo test --test measure measure_writes_verification_artifact -- --ignored
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn measure_writes_verification_artifact() {
    let out = PathBuf::from("../Sample PDFs/vibepdf-verify-measure.pdf");
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).expect("ensure Sample PDFs dir");
    }
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    handle
        .add_measure(0, "distance".into(), vec![[90.0, 700.0], [320.0, 700.0]], "#1f6feb".into(), "4.60 m".into(), 1.0, 1.5, 0.02, "m".into())
        .await
        .expect("distance");
    handle
        .add_measure(0, "area".into(), vec![[100.0, 560.0], [300.0, 560.0], [320.0, 460.0], [120.0, 440.0]], "#1e8449".into(), "3.2 m²".into(), 1.0, 1.5, 0.02, "m".into())
        .await
        .expect("area");
    handle.save(Some(out.clone())).await.expect("save");
    eprintln!("wrote measurement verification artifact to {}", out.display());

    drop(handle);
}
