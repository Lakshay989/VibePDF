//! Integration tests for XFDF import/export (P3.E1).
//!
//! SPEC: P3-ANN-010 — export every annotation to an XFDF sidecar and import one
//! back, restored identically (semantic identity: kind, geometry, contents, and
//! reply links — not byte-equal `/AP`). Through the actor against `hello.pdf`.

use std::path::PathBuf;

use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::cos::read_annotations;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn temp_subdir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("vibepdf-xfdf-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

fn spawn() -> DocumentActorHandle {
    let id = uuid::Uuid::new_v4();
    DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn")
}

/// Build a representative annotation of (almost) every supported subtype on page
/// 0: a highlight, a filled rectangle, an arrow, an ink stroke, a distance
/// measurement, and a note with a reply. Returns the count created (7).
async fn build_standard_set(handle: &DocumentActorHandle) -> usize {
    handle
        .add_text_markup(0, "highlight".into(), vec![[100.0, 700.0, 200.0, 700.0, 100.0, 680.0, 200.0, 680.0]], "#ffff00".into(), 1.0)
        .await
        .expect("highlight");
    handle
        .add_shape(0, "rectangle".into(), [120.0, 500.0, 260.0, 580.0], "#ff0000".into(), Some("#00ff00".into()), 0.8, 2.0)
        .await
        .expect("square");
    handle
        .add_line(0, 100.0, 400.0, 300.0, 420.0, true, "#0000ff".into(), 1.0, 1.5)
        .await
        .expect("line");
    handle
        .add_ink(0, vec![[100.0, 300.0, 0.5], [120.0, 320.0, 0.5], [140.0, 300.0, 0.5]], "#101010".into(), 1.0, 2.0)
        .await
        .expect("ink");
    handle
        .add_measure(0, "distance".into(), vec![[100.0, 200.0], [260.0, 200.0]], "#ff00ff".into(), "160 pt".into(), 1.0, 1.0)
        .await
        .expect("measure");
    handle
        .add_note("root".into(), 0, 130.0, 650.0, "Please review this.".into(), "Ada".into())
        .await
        .expect("note");
    handle
        .add_reply("root".into(), "Bo".into(), "Looks good to me.".into())
        .await
        .expect("reply");
    7
}

async fn export_to_string(handle: &DocumentActorHandle, dir: &std::path::Path) -> (String, usize) {
    let out = dir.join("annots.xfdf");
    let count = handle.export_annotations(out.clone()).await.expect("export");
    let xml = std::fs::read_to_string(&out).expect("read xfdf");
    (xml, count)
}

#[tokio::test]
async fn export_contains_every_subtype() {
    let dir = temp_subdir();
    let handle = spawn();
    let n = build_standard_set(&handle).await;
    let (xml, count) = export_to_string(&handle, &dir).await;

    assert_eq!(count, n, "export reports one entry per annotation");
    for tag in ["<highlight", "<square", "<line", "<ink", "<text"] {
        assert!(xml.contains(tag), "XFDF should contain a {tag} element:\n{xml}");
    }
    // The measurement keeps its dimension intent so it re-imports as a measure.
    assert!(xml.contains("it=\"LineDimension\""), "measure carries its /IT intent");
    // The reply links to its parent and is tagged as a reply.
    assert!(xml.contains("inreplyto=\"root\""), "reply links to its parent /NM");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn roundtrip_preserves_geometry_and_contents() {
    let dir = temp_subdir();
    let source = spawn();
    let n = build_standard_set(&source).await;
    let (xml, _) = export_to_string(&source, &dir).await;

    // Import onto a clean copy of the same base PDF (no annotations yet).
    let target = spawn();
    let before = read_annotations(&target.get_bytes().await.expect("bytes")).expect("read");
    assert_eq!(before.len(), 0, "the fresh document starts with no annotations");

    target.import_xfdf(xml).await.expect("import");
    let after = read_annotations(&target.get_bytes().await.expect("bytes")).expect("read");

    assert_eq!(after.len(), n, "every exported annotation is recreated");

    let mut kinds: Vec<String> = after.iter().map(|a| a.kind.clone()).collect();
    kinds.sort();
    // note + reply are both /Text, so "note" appears twice.
    let expected: Vec<String> = ["highlight", "ink", "line", "measure", "note", "note", "rectangle"]
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    assert_eq!(kinds, expected, "kinds round-trip");

    // Geometry: the rectangle's rect survives within rounding.
    let rect = after.iter().find(|a| a.kind == "rectangle").expect("rectangle");
    let near = |a: f32, b: f32| (a - b).abs() < 0.5;
    assert!(near(rect.rect[0], 120.0) && near(rect.rect[3], 580.0), "rect geometry preserved: {:?}", rect.rect);

    // Contents survive on the note + the measurement label.
    assert!(after.iter().any(|a| a.contents == "Please review this."), "note text preserved");
    assert!(after.iter().any(|a| a.contents == "160 pt"), "measure label preserved");

    drop(source);
    drop(target);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn roundtrip_preserves_reply_thread() {
    let dir = temp_subdir();
    let source = spawn();
    source.add_note("root".into(), 0, 130.0, 650.0, "Parent".into(), "Ada".into()).await.expect("note");
    source.add_reply("root".into(), "Bo".into(), "Child".into()).await.expect("reply");
    let (xml, _) = export_to_string(&source, &dir).await;

    let target = spawn();
    target.import_xfdf(xml).await.expect("import");
    let after = read_annotations(&target.get_bytes().await.expect("bytes")).expect("read");

    assert_eq!(after.len(), 2, "note + reply recreated");
    let reply = after.iter().find(|a| a.contents == "Child").expect("reply");
    assert_eq!(reply.in_reply_to.as_deref(), Some("root"), "reply still linked to its parent /NM");

    drop(source);
    drop(target);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn import_is_one_undoable_edit() {
    let dir = temp_subdir();
    let source = spawn();
    let n = build_standard_set(&source).await;
    let (xml, _) = export_to_string(&source, &dir).await;

    let target = spawn();
    let state = target.import_xfdf(xml).await.expect("import");
    assert!(state.can_undo, "an import is undoable");
    let mid = read_annotations(&target.get_bytes().await.expect("bytes")).expect("read");
    assert_eq!(mid.len(), n, "import added every annotation");

    // A single undo removes the whole import.
    target.undo().await.expect("undo");
    let after = read_annotations(&target.get_bytes().await.expect("bytes")).expect("read");
    assert_eq!(after.len(), 0, "one undo reverses the entire import");

    drop(source);
    drop(target);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Writes an annotated PDF + its exported XFDF to the git-ignored `Sample PDFs/`
/// for the manual cross-reader ritual. Ignored; run on demand:
///   cargo test --test xfdf_roundtrip xfdf_writes_verification_artifact -- --ignored
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn xfdf_writes_verification_artifact() {
    let dir = PathBuf::from("../Sample PDFs");
    std::fs::create_dir_all(&dir).expect("ensure Sample PDFs dir");
    let handle = spawn();
    build_standard_set(&handle).await;
    handle.save(Some(dir.join("vibepdf-verify-xfdf.pdf"))).await.expect("save pdf");
    let count = handle.export_annotations(dir.join("vibepdf-verify-xfdf.xfdf")).await.expect("export");
    eprintln!("wrote {count}-annotation verification artifact to {}", dir.display());
    drop(handle);
}
