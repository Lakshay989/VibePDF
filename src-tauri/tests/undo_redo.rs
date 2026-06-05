//! Integration tests for the undo/redo actor messages (P2.A3).
//!
//! SPEC: P2-PAGE-003 / session history. P2.A3 ships the stack machinery
//! only — no page operation exists yet to record onto it — so these
//! tests assert the actor-level wiring: a freshly opened document has an
//! empty history, and undo/redo on an empty stack are no-ops reporting
//! the unchanged availability. The full page-tree round-trip ("delete,
//! undo three times, page tree identical") lands with P2.B2 (delete),
//! which registers real `Edit<PdfDocument>` actions. The stack mechanics
//! themselves are unit-tested in `src/pdf/undo.rs`.

use std::path::PathBuf;

use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::undo::HistoryState;

fn hello_pdf() -> PathBuf {
    // Test runs with CWD = src-tauri/.
    let p = PathBuf::from("../tests/fixtures/basic/hello.pdf");
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

#[tokio::test]
async fn fresh_document_has_empty_history() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, hello_pdf(), None).expect("spawn");
    assert_eq!(
        handle.history_state().await.expect("history_state round-trip"),
        HistoryState {
            can_undo: false,
            can_redo: false,
        },
    );
}

#[tokio::test]
async fn undo_and_redo_on_empty_stack_are_noops() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, hello_pdf(), None).expect("spawn");

    assert_eq!(
        handle.undo().await.expect("undo round-trip"),
        HistoryState::default(),
    );
    assert_eq!(
        handle.redo().await.expect("redo round-trip"),
        HistoryState::default(),
    );

    // A no-op undo/redo must not disturb the document.
    assert_eq!(handle.page_count().await.expect("page_count"), 1);
}
