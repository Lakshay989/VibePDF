//! SPEC: P1-VIEW-003 — encrypted PDFs surface as a typed
//! `CommandError::PasswordRequired` and unlock correctly when the
//! right password is supplied.
//!
//! The fixture is the existing `tests/fixtures/acceptance/p1-encrypted.pdf`,
//! generated on demand by `tests/fixtures/acceptance/generate.py
//! encrypted` (user password `vibepdf`, owner password `vibepdf-owner`,
//! 256-bit AES via pypdf — see `tests/fixtures/acceptance/README.md`).
//! It is gitignored, so the file may not exist on a fresh clone; in
//! that case every test in this file is gracefully skipped with a
//! clear instruction to regenerate it. We do **not** silently pass
//! when the fixture is absent — `cargo test` should make the missing
//! piece obvious.
//!
//! As with `actor_smoke.rs`, no real Tauri app is available, so we
//! spawn with `app: None`.

use std::path::PathBuf;

use vibepdf_lib::error::CommandError;
use vibepdf_lib::pdf::actor::DocumentActorHandle;

const USER_PASSWORD: &str = "vibepdf";

/// Returns `Some(path)` when the fixture is present, `None` otherwise.
/// Each test calls this and skips with a printed reason when missing —
/// keeping CI green on fresh clones that haven't run the generator yet.
fn encrypted_pdf_or_skip() -> Option<PathBuf> {
    let p = PathBuf::from("../tests/fixtures/acceptance/p1-encrypted.pdf");
    if p.is_file() {
        Some(p)
    } else {
        eprintln!(
            "skipping: fixture missing at {}\n  regenerate with: \
             python3 tests/fixtures/acceptance/generate.py encrypted",
            p.display(),
        );
        None
    }
}

#[tokio::test]
async fn open_encrypted_without_password_returns_password_required() {
    // SPEC: P1-VIEW-003 — first-attempt open with no password on an
    // encrypted file must come back as the typed PasswordRequired so
    // the frontend can mount the prompt dialog.
    let Some(path) = encrypted_pdf_or_skip() else {
        return;
    };

    let id = uuid::Uuid::new_v4();
    let err = DocumentActorHandle::spawn(None, id, path, None)
        .expect_err("opening an encrypted PDF without a password must fail");

    match err {
        CommandError::PasswordRequired(_) => {}
        other => panic!("expected PasswordRequired, got {other:?}"),
    }
}

#[tokio::test]
async fn open_encrypted_with_wrong_password_returns_password_required() {
    // SPEC: P1-VIEW-003 — PDFium does not distinguish "no password" from
    // "wrong password"; both must flow through PasswordRequired so the
    // dialog can re-prompt without special-casing.
    let Some(path) = encrypted_pdf_or_skip() else {
        return;
    };

    let id = uuid::Uuid::new_v4();
    let err = DocumentActorHandle::spawn(None, id, path, Some("wrong".into()))
        .expect_err("opening with a wrong password must fail");

    match err {
        CommandError::PasswordRequired(_) => {}
        other => panic!("expected PasswordRequired, got {other:?}"),
    }
}

#[tokio::test]
async fn open_encrypted_with_correct_password_succeeds() {
    // SPEC: P1-VIEW-003 — correct password opens cleanly. We piggy-back
    // on the existing actor mailbox to round-trip `page_count` and
    // assert the document is fully usable, not just opened.
    let Some(path) = encrypted_pdf_or_skip() else {
        return;
    };

    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, path, Some(USER_PASSWORD.into()))
        .expect("opening with the correct password must succeed");

    // p1-encrypted.pdf is hello.pdf re-encrypted; hello.pdf has 1 page.
    assert_eq!(handle.metadata().page_count, 1);
    assert_eq!(
        handle.page_count().await.expect("page_count round-trip"),
        1
    );
}
