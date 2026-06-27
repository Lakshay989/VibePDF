//! Integration tests for click-to-edit text (P4.B1) at the actor layer.
//!
//! SPEC: P4-EDIT-001 — editing a run rewrites its text in the saved document,
//! is undoable, and preserves the rest. Exercised through the document actor's
//! `ReplaceTextRun` message (the same path the IPC command drives), verifying by
//! A1 text-run re-extraction.

use std::path::PathBuf;

use vibepdf_lib::pdf::actor::DocumentActorHandle;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn spawn(name: &str) -> DocumentActorHandle {
    let id = uuid::Uuid::new_v4();
    DocumentActorHandle::spawn(None, id, fixture(name), None).expect("spawn")
}

async fn page0_text(handle: &DocumentActorHandle) -> String {
    handle
        .read_text_runs(0)
        .await
        .expect("read runs")
        .iter()
        .map(|r| r.text.as_str())
        .collect()
}

#[tokio::test]
async fn replace_changes_text_and_records_undo() {
    let handle = spawn("hello.pdf");
    assert!(page0_text(&handle).await.contains("VibePDF"), "fixture sanity");

    let state = handle
        .replace_text_run(0, 0, "Hello, Acrobat.".to_owned())
        .await
        .expect("replace");
    assert!(state.can_undo, "an edit must be undoable");

    let after = page0_text(&handle).await;
    assert!(after.contains("Hello, Acrobat."), "new text present: {after:?}");
    assert!(!after.contains("VibePDF"), "old text gone: {after:?}");
    drop(handle);
}

#[tokio::test]
async fn undo_restores_original_text() {
    let handle = spawn("hello.pdf");
    handle
        .replace_text_run(0, 0, "Edited!".to_owned())
        .await
        .expect("replace");

    handle.undo().await.expect("undo");
    let restored = page0_text(&handle).await;
    assert!(restored.contains("VibePDF"), "undo restores original: {restored:?}");
    drop(handle);
}

#[tokio::test]
async fn out_of_range_run_index_errors() {
    let handle = spawn("hello.pdf");
    assert!(
        handle.replace_text_run(0, 99, "x".to_owned()).await.is_err(),
        "editing a non-existent run is rejected, not a panic"
    );
    drop(handle);
}

#[tokio::test]
async fn delete_then_undo_restores() {
    let handle = spawn("hello.pdf");
    assert!(page0_text(&handle).await.contains("VibePDF"), "fixture sanity");

    let state = handle.delete_text_run(0, 0).await.expect("delete");
    assert!(state.can_undo, "a delete must be undoable");
    assert!(!page0_text(&handle).await.contains("VibePDF"), "run removed");

    handle.undo().await.expect("undo");
    assert!(page0_text(&handle).await.contains("VibePDF"), "undo restores the run");
    drop(handle);
}

/// Drives the full B1 path — actor edit then save — and writes the result to /tmp
/// for the manual three-reader check. Ignored by default (produces an artifact).
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn writes_verification_artifact() {
    let handle = spawn("hello.pdf");
    handle
        .replace_text_run(0, 0, "Hello, World!".to_owned())
        .await
        .expect("replace");
    handle
        .save(Some(PathBuf::from("/tmp/vibepdf-verify.pdf")))
        .await
        .expect("save-as");
    drop(handle);
}
