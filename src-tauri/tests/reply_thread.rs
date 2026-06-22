//! Integration tests for reply threads (P3.D2).
//!
//! SPEC: P3-ANN-009 — a reply is a `/Text` linked to its parent via `/IRT`,
//! persisted through the PDFium save round-trip and undoable. Through the actor
//! against `hello.pdf`.

use std::path::PathBuf;

use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::cos::read_annotations;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn temp_subdir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("vibepdf-reply-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

#[tokio::test]
async fn reply_persists_and_links_to_parent() {
    let dir = temp_subdir();
    let out = dir.join("reply.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    handle
        .add_note("parent".into(), 0, 120.0, 650.0, "any thoughts?".into(), "Ada".into())
        .await
        .expect("note");
    let state = handle
        .add_reply("parent".into(), "Bo".into(), "looks good to me".into())
        .await
        .expect("reply");
    assert!(state.can_undo, "a reply must be undoable");

    handle.save(Some(out.clone())).await.expect("save");
    let bytes = std::fs::read(&out).expect("read");
    let infos = read_annotations(&bytes).expect("annotations");
    assert_eq!(infos.len(), 2, "the note + its reply survive the round-trip");
    let reply = infos.iter().find(|a| a.contents == "looks good to me").expect("reply");
    assert_eq!(reply.in_reply_to.as_deref(), Some("parent"), "linked via /IRT");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn reply_undo_removes_only_the_reply() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    handle
        .add_note("p".into(), 0, 120.0, 650.0, "q".into(), "Ada".into())
        .await
        .expect("note");
    handle.add_reply("p".into(), "Bo".into(), "a".into()).await.expect("reply");
    let after_undo = handle.undo().await.expect("undo");
    assert!(after_undo.can_redo, "undo of add-reply enables redo");

    let bytes = handle.get_bytes().await.expect("bytes");
    let infos = read_annotations(&bytes).expect("annotations");
    assert_eq!(infos.len(), 1, "only the parent note remains");
    assert_eq!(infos[0].in_reply_to, None);

    drop(handle);
}

#[tokio::test]
async fn reply_rejects_unknown_parent() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");
    let err = handle.add_reply("ghost".into(), "Bo".into(), "x".into()).await;
    assert!(err.is_err(), "replying to a missing parent is rejected");
    drop(handle);
}

/// Writes a reply-thread PDF to the git-ignored `Sample PDFs/` for the manual
/// cross-reader ritual. Ignored; run on demand:
///   cargo test --test reply_thread reply_writes_verification_artifact -- --ignored
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn reply_writes_verification_artifact() {
    let out = PathBuf::from("../Sample PDFs/vibepdf-verify-reply.pdf");
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).expect("ensure Sample PDFs dir");
    }
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    handle
        .add_note("root".into(), 0, 120.0, 650.0, "Please review this section.".into(), "Ada".into())
        .await
        .expect("note");
    handle.add_reply("root".into(), "Bo".into(), "Reviewed — looks good.".into()).await.expect("r1");
    handle.add_reply("root".into(), "Ada".into(), "Thanks!".into()).await.expect("r2");
    handle.save(Some(out.clone())).await.expect("save");
    eprintln!("wrote reply verification artifact to {}", out.display());

    drop(handle);
}
