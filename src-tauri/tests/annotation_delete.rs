//! Integration tests for select + delete annotations (P3.D1 management).
//!
//! SPEC: P3-ANN-012 — any annotation read from the sidebar can be deleted by its
//! handle as an undoable edit. Exercised through the actor against `hello.pdf`.

use std::path::PathBuf;

use vibepdf_lib::pdf::actor::DocumentActorHandle;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

#[tokio::test]
async fn deletes_each_kind_through_actor() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    let quads = vec![[72.0_f32, 700.0, 200.0, 700.0, 72.0, 688.0, 200.0, 688.0]];
    handle.add_text_markup(0, "highlight".into(), quads, "#ffd400".into(), 1.0).await.expect("markup");
    handle.add_note("n".into(), 0, 100.0, 600.0, "hi".into(), "Ada".into()).await.expect("note");
    handle
        .add_free_text(0, [50.0, 400.0, 250.0, 440.0], "boxed".into(), "Helvetica".into(), 12.0, "#000000".into(), false, false, false)
        .await
        .expect("free text");
    handle
        .add_shape(0, "rectangle".into(), [60.0, 200.0, 260.0, 300.0], "#000000".into(), None, 1.0, 2.0)
        .await
        .expect("shape");

    let infos = handle.read_annotations().await.expect("read");
    assert_eq!(infos.len(), 4, "highlight + note + free-text + shape");

    // Delete each one by its handle; the list shrinks by one each time.
    let mut remaining = infos.len();
    for info in infos {
        handle.delete_annotation(info.id.clone()).await.expect("delete");
        remaining -= 1;
        assert_eq!(handle.read_annotations().await.expect("re-read").len(), remaining, "after deleting {}", info.id);
    }
    assert!(handle.read_annotations().await.expect("final").is_empty());

    drop(handle);
}

#[tokio::test]
async fn delete_is_undoable() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    handle
        .add_shape(0, "ellipse".into(), [60.0, 200.0, 260.0, 300.0], "#0000ff".into(), None, 1.0, 2.0)
        .await
        .expect("shape");
    let info = handle.read_annotations().await.expect("read").remove(0);

    handle.delete_annotation(info.id).await.expect("delete");
    assert!(handle.read_annotations().await.expect("after delete").is_empty());

    handle.undo().await.expect("undo");
    assert_eq!(handle.read_annotations().await.expect("after undo").len(), 1, "undo restores the shape");

    drop(handle);
}
