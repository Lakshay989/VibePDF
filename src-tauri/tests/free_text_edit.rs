//! Integration tests for editing a free-text annotation in place (P3-ANN-013).
//!
//! Exercised through the actor against `hello.pdf`: add → read the editable
//! state → update → re-read; undo restores.

use std::path::PathBuf;

use vibepdf_lib::pdf::actor::DocumentActorHandle;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

#[tokio::test]
async fn edit_round_trips_through_actor() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    handle
        .add_free_text(0, [100.0, 600.0, 320.0, 640.0], "Helo".into(), "Times".into(), 18.0, "#cc1133".into(), true, false, false)
        .await
        .expect("add");
    let nm = handle.read_annotations().await.expect("read")[0].id.clone();

    // The editor reads the existing state.
    let before = handle.read_free_text(nm.clone()).await.expect("read ft").expect("some");
    assert_eq!(before.text, "Helo");
    assert_eq!(before.font_family, "Times");
    assert!(before.bold);

    // Fix the typo + restyle.
    handle
        .update_free_text(nm.clone(), "Hello".into(), "Helvetica".into(), 24.0, "#003399".into(), false, true, false)
        .await
        .expect("update");

    let after = handle.read_free_text(nm.clone()).await.expect("read ft").expect("some");
    assert_eq!(after.text, "Hello");
    assert_eq!(after.font_family, "Helvetica");
    assert!(after.italic && !after.bold);
    assert!((after.font_size - 24.0).abs() < 0.5);
    // Same annotation (identity preserved): still exactly one, same handle.
    let ids: Vec<String> =
        handle.read_annotations().await.expect("read").into_iter().map(|a| a.id).collect();
    assert_eq!(ids, vec![nm.clone()]);

    // Undo restores the original text.
    handle.undo().await.expect("undo");
    assert_eq!(handle.read_free_text(nm).await.expect("read").expect("some").text, "Helo");

    drop(handle);
}

#[tokio::test]
async fn read_free_text_none_for_non_free_text() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");
    // A note is not a free-text — read_free_text returns None for its handle.
    handle.add_note("note-nm".into(), 0, 100.0, 700.0, "hi".into(), "A".into()).await.expect("note");
    assert!(handle.read_free_text("note-nm".into()).await.expect("read").is_none());
    drop(handle);
}
