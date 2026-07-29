//! SPEC: NFR-PERF-005 — the actor's read cache must stay consistent: a read
//! reflects the latest edit (invalidated on write) and reverts after undo
//! (invalidated on restore). If the cache ever served a stale parse, these
//! reads would return the wrong set.

use std::path::PathBuf;

use vibepdf_lib::pdf::actor::DocumentActorHandle;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

#[tokio::test]
async fn read_cache_invalidates_on_edit_and_undo() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    // Cold read — no boxes yet. (Fills the cache.)
    let before = handle.read_text_boxes(0).await.expect("read empty");
    assert_eq!(before.len(), 0, "fresh document has no text boxes");

    // Write an edit; the cache must invalidate so the next read re-parses.
    handle
        .add_text_box(
            0,
            [72.0, 500.0, 320.0, 640.0],
            "cache test".into(),
            "Helvetica".into(),
            16.0,
            "#000000".into(),
            false,
            false,
            false,
        )
        .await
        .expect("add text box");

    // Read again (a second read exercises warm-cache reuse too): sees the edit.
    let after_add = handle.read_text_boxes(0).await.expect("read after add");
    let after_add_2 = handle.read_text_boxes(0).await.expect("read again");
    assert_eq!(after_add.len(), 1, "read must reflect the committed edit");
    assert_eq!(after_add_2.len(), 1, "warm-cache read must agree");
    assert_eq!(after_add[0].text, "cache test");

    // Undo restores the pre-edit bytes; the cache must invalidate again.
    handle.undo().await.expect("undo");
    let after_undo = handle.read_text_boxes(0).await.expect("read after undo");
    assert_eq!(after_undo.len(), 0, "read must revert after undo (cache re-parsed)");

    drop(handle);
}
