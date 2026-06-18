//! Integration tests for the annotation sidebar read path (P3.D1).
//!
//! SPEC: P3-ANN-008 — read every supported annotation back out of the live
//! document so the frontend can list them. Exercised through the actor.

use std::path::PathBuf;

use vibepdf_lib::pdf::actor::DocumentActorHandle;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

#[tokio::test]
async fn reads_all_through_actor() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    handle.add_note("n".into(), 0, 100.0, 700.0, "hi".into(), "Ada".into()).await.expect("note");
    handle
        .add_free_text(0, [50.0, 400.0, 250.0, 440.0], "boxed".into(), "Helvetica".into(), 12.0, "#000000".into(), false, false)
        .await
        .expect("free text");

    let infos = handle.read_annotations().await.expect("read");
    assert_eq!(infos.len(), 2, "the note + the free-text");
    let kinds: Vec<&str> = infos.iter().map(|a| a.kind.as_str()).collect();
    assert!(kinds.contains(&"note") && kinds.contains(&"freetext"), "{kinds:?}");

    drop(handle);
}

#[tokio::test]
async fn read_annotations_reflects_undo() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    handle.add_note("n".into(), 0, 100.0, 700.0, "hi".into(), "Ada".into()).await.expect("note");
    assert_eq!(handle.read_annotations().await.expect("read").len(), 1);

    handle.undo().await.expect("undo");
    assert!(handle.read_annotations().await.expect("read").is_empty(), "undo cleared the note");

    drop(handle);
}

#[tokio::test]
async fn read_annotations_empty_on_plain_pdf() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");
    assert!(handle.read_annotations().await.expect("read").is_empty());
    drop(handle);
}

/// Writes a PDF carrying one of each surfaced kind (a highlight, a note, a
/// free-text box) to the git-ignored `Sample PDFs/` for the D1 sidebar demo.
/// Ignored; run on demand:
///   cargo test --test read_annotations writes_sidebar_demo_artifact -- --ignored
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn writes_sidebar_demo_artifact() {
    let out = PathBuf::from("../Sample PDFs/vibepdf-verify-annots.pdf");
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).expect("ensure Sample PDFs dir");
    }
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    let quads = vec![[72.0_f32, 700.0, 260.0, 700.0, 72.0, 686.0, 260.0, 686.0]];
    handle
        .add_text_markup(0, "highlight".into(), quads, "#ffd400".into(), 1.0)
        .await
        .expect("highlight");
    handle
        .add_note("demo".into(), 0, 300.0, 650.0, "A sticky note for the sidebar.".into(), "VibePDF User".into())
        .await
        .expect("note");
    handle
        .add_free_text(0, [80.0, 540.0, 360.0, 600.0], "A free-text box.".into(), "Times".into(), 16.0, "#1133cc".into(), true, false)
        .await
        .expect("free text");

    handle.save(Some(out.clone())).await.expect("save");
    let infos = handle.read_annotations().await.expect("read");
    assert_eq!(infos.len(), 3, "highlight + note + free-text");
    eprintln!("wrote sidebar demo artifact to {}", out.display());

    drop(handle);
}
