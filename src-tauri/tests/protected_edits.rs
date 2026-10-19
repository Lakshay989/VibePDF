//! SPEC: P1-VIEW-003 — an edit that goes through a lopdf round trip refuses a
//! protected document instead of damaging it.
//!
//! Before this, those edits did one of three things depending on the file
//! (measured 2026-09-13):
//!
//! - a document needing a password to open: failed with a bare
//!   `PasswordRequired`;
//! - an RC4 permissions-only file lopdf could not authenticate: "succeeded",
//!   and the new content saved as garbage;
//! - an RC4 permissions-only file lopdf *could* decrypt: "succeeded", and the
//!   next save wrote the file with its protection silently removed — lopdf
//!   decrypted it on load and wrote the edit back without `/Encrypt`.
//!
//! The last is the dangerous one, and it is what these tests are built around:
//! they save after trying to edit and check the protection is still on disk.

use std::path::{Path, PathBuf};

use lopdf::{EncryptionState, EncryptionVersion, Object, Permissions, StringFormat};
use vibepdf_lib::error::CommandError;
use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::clean::CleanOptions;
use vibepdf_lib::pdf::document::{declares_encryption, PROTECTED_EDIT_REFUSAL};
use vibepdf_lib::security::encrypt::{encrypt_document, DocumentPermissions, EncryptOptions};
use vibepdf_lib::security::redact::RedactOptions;

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("vibepdf-protected-edits-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Self(dir)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.path(name);
        std::fs::write(&path, bytes).expect("write");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn hello() -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/basic/hello.pdf")).expect("hello.pdf")
}

/// Protected the way VibePDF's Protect does it: AES-256, a password to open.
fn needs_a_password() -> Vec<u8> {
    let opts = EncryptOptions {
        user_password: Some("open-me".into()),
        owner_password: None,
        permissions: DocumentPermissions::default(),
    };
    encrypt_document(&hello(), &opts).expect("encrypt")
}

/// Protected the way many other tools do it: RC4, no password to open, an
/// owner password set. lopdf decrypts this kind on load — the case where an
/// edit used to strip the protection without a word.
fn rc4_permissions_only() -> Vec<u8> {
    let mut doc = lopdf::Document::load_mem(&hello()).expect("load");
    let file_id = Object::String(b"0123456789abcdef".to_vec(), StringFormat::Hexadecimal);
    doc.trailer.set("ID", Object::Array(vec![file_id.clone(), file_id]));
    let state = EncryptionState::try_from(EncryptionVersion::V2 {
        document: &doc,
        owner_password: "owner-rc4",
        user_password: "",
        key_length: 128,
        permissions: Permissions::all(),
    })
    .expect("RC4 state");
    doc.encrypt(&state).expect("encrypt");
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("save");
    assert!(declares_encryption(&out), "the fixture itself must be protected");
    out
}

fn open(path: PathBuf, password: Option<&str>) -> DocumentActorHandle {
    DocumentActorHandle::spawn(None, uuid::Uuid::new_v4(), path, password.map(str::to_string)).expect("opens")
}

/// One edit from each family that goes through a lopdf round trip.
async fn round_trip_edits(h: &DocumentActorHandle) -> Vec<(&'static str, Result<(), CommandError>)> {
    let id = || uuid::Uuid::new_v4().to_string();
    vec![
        ("add a note", h.add_note(id(), 0, 100.0, 600.0, "x".into(), "me".into()).await.map(drop)),
        ("add a shape", h.add_shape(0, "rectangle".into(), [100.0, 100.0, 200.0, 200.0], "#000000".into(), None, 1.0, 1.0).await.map(drop)),
        ("add a comment box", h.add_free_text(0, [100.0, 100.0, 300.0, 140.0], "x".into(), "Helvetica".into(), 12.0, "#000000".into(), false, false, false).await.map(drop)),
        ("add a text box", h.add_text_box(0, [100.0, 100.0, 300.0, 140.0], "x".into(), "Helvetica".into(), 12.0, "#000000".into(), false, false, false).await.map(drop)),
        ("resize a page", h.resize_pages(vec![0], 500.0, 700.0, true).await.map(drop)),
        ("clean metadata", h.clean_document(CleanOptions { metadata: true, ..Default::default() }).await.map(drop)),
        ("redact a region", h.redact_region(0, [100.0, 700.0, 300.0, 730.0], RedactOptions { remove_metadata: false }).await.map(drop)),
        ("flatten annotations", h.flatten_annotations().await.map(drop)),
        ("add a form field", h.add_text_field(0, [100.0, 100.0, 300.0, 130.0], "f1".into(), String::new(), None, false, false).await.map(drop)),
    ]
}

fn assert_all_refused(label: &str, results: Vec<(&'static str, Result<(), CommandError>)>) {
    for (what, result) in results {
        match result {
            Err(CommandError::InvalidInput(message)) if message == PROTECTED_EDIT_REFUSAL => {}
            Err(other) => panic!("{label}: \"{what}\" failed with the wrong error: {other:?}"),
            Ok(()) => panic!("{label}: \"{what}\" was allowed on a protected document"),
        }
    }
}

#[tokio::test]
async fn round_trip_edits_refuse_a_document_that_needs_a_password() {
    let scratch = Scratch::new("aes");
    let handle = open(scratch.write("protected.pdf", &needs_a_password()), Some("open-me"));
    assert_all_refused("AES-256, open password", round_trip_edits(&handle).await);
}

// The dangerous case. Every edit here used to go through, and saving
// afterwards wrote the file without its protection.
#[tokio::test]
async fn a_permissions_only_document_keeps_its_protection_through_attempted_edits() {
    let scratch = Scratch::new("rc4");
    let handle = open(scratch.write("protected.pdf", &rc4_permissions_only()), None);
    assert_all_refused("RC4, permissions only", round_trip_edits(&handle).await);

    let saved = scratch.path("saved.pdf");
    handle.save(Some(saved.clone())).await.expect("save");
    assert!(
        declares_encryption(&std::fs::read(&saved).expect("read saved")),
        "saving after refused edits must still write the protection"
    );
}

#[tokio::test]
async fn a_refused_edit_leaves_nothing_to_undo() {
    let scratch = Scratch::new("undo");
    let handle = open(scratch.write("protected.pdf", &rc4_permissions_only()), None);
    let _ = round_trip_edits(&handle).await;
    assert!(!handle.history_state().await.expect("history").can_undo, "a refused edit must not be recorded");
}

// Rotation is PDFium-native: PDFium re-encrypts on save, so it is safe and
// must keep working.
#[tokio::test]
async fn native_edits_still_work_and_keep_the_protection() {
    for (label, bytes, password) in [
        ("AES-256, open password", needs_a_password(), Some("open-me")),
        ("RC4, permissions only", rc4_permissions_only(), None),
    ] {
        let scratch = Scratch::new("native");
        let handle = open(scratch.write("protected.pdf", &bytes), password);
        handle.rotate_pages(vec![0], 1).await.unwrap_or_else(|e| panic!("{label}: rotate refused: {e:?}"));
        let saved = scratch.path("saved.pdf");
        handle.save(Some(saved.clone())).await.expect("save");
        assert!(declares_encryption(&std::fs::read(&saved).expect("read")), "{label}: rotation must not strip protection");
        assert_eq!(open(saved, password).metadata().page_count, 1, "{label}: the rotated file must reopen");
    }
}

// Guards against the gate being too eager: an ordinary document is untouched.
#[tokio::test]
async fn an_unprotected_document_is_still_editable() {
    let scratch = Scratch::new("plain");
    let handle = open(scratch.write("hello.pdf", &hello()), None);
    for (what, result) in round_trip_edits(&handle).await {
        if let Err(CommandError::InvalidInput(message)) = &result {
            assert_ne!(message, PROTECTED_EDIT_REFUSAL, "\"{what}\" was refused on an unprotected document");
        }
    }
}
