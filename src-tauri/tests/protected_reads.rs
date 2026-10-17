//! SPEC: P1-VIEW-003 — a protected document's panels show what is on the page,
//! and lopdf never quietly works on bytes it could not decrypt.
//!
//! The notes, text-box and form-field panels are lopdf reads. lopdf cannot
//! decrypt a document that needs an open password, and it does not say so:
//! 0.36 hands back the objects with every string still encrypted, and 0.42
//! hands back a document with no objects at all. So a protected document's
//! notes read as ciphertext, or as nothing — and a file picked as a merge input
//! or a page background was imported as ciphertext without a word.
//!
//! The reads now see the same decrypted copy the screen does (held in memory,
//! never saved), and a source lopdf cannot decrypt is refused by name.

use std::path::{Path, PathBuf};

use lopdf::{EncryptionState, EncryptionVersion, Object, Permissions, StringFormat};
use vibepdf_lib::error::CommandError;
use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::background::BackgroundKind;
use vibepdf_lib::pdf::document::{declares_encryption, NEEDS_PASSWORD, PROTECTED_EDIT_REFUSAL};
use vibepdf_lib::pdf::merge::merge_documents;
use vibepdf_lib::security::encrypt::{encrypt_document, DocumentPermissions, EncryptOptions};

const PASSWORD: &str = "open-me";

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("vibepdf-protected-reads-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Self(dir)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn hello_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/basic/hello.pdf")
}

fn open(path: PathBuf, password: Option<&str>) -> DocumentActorHandle {
    DocumentActorHandle::spawn(None, uuid::Uuid::new_v4(), path, password.map(str::to_string)).expect("opens")
}

fn protect(bytes: &[u8], user: Option<&str>, owner: Option<&str>) -> Vec<u8> {
    let opts = EncryptOptions {
        user_password: user.map(str::to_string),
        owner_password: owner.map(str::to_string),
        permissions: DocumentPermissions::default(),
    };
    let out = encrypt_document(bytes, &opts).expect("encrypt");
    assert!(declares_encryption(&out), "the fixture itself must be protected");
    out
}

/// `hello.pdf` with one sticky note and one filled text field, saved
/// unprotected — the content the protected copies are made from.
async fn annotated_hello(scratch: &Scratch) -> Vec<u8> {
    let handle = open(hello_path(), None);
    handle
        .add_note("note-1".into(), 0, 100.0, 600.0, "Meet at noon".into(), "Ana".into())
        .await
        .expect("add note");
    handle
        .add_text_field(0, [100.0, 400.0, 300.0, 430.0], "email".into(), "ana@example.com".into(), None, false, false)
        .await
        .expect("add field");
    let dest = scratch.path("annotated.pdf");
    handle.save(Some(dest.clone())).await.expect("save");
    std::fs::read(dest).expect("read annotated")
}

async fn open_protected(scratch: &Scratch, user: Option<&str>, owner: Option<&str>) -> DocumentActorHandle {
    let path = scratch.path("protected.pdf");
    std::fs::write(&path, protect(&annotated_hello(scratch).await, user, owner)).expect("write");
    open(path, user)
}

/// Protected the way many other tools do it: RC4, an owner password, and no
/// password to open. VibePDF's own Protect won't write this kind, so lopdf
/// builds it.
fn rc4_permissions_only(bytes: &[u8]) -> Vec<u8> {
    let mut doc = lopdf::Document::load_mem(bytes).expect("load");
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

async fn open_permissions_only(scratch: &Scratch) -> DocumentActorHandle {
    let path = scratch.path("permissions-only.pdf");
    std::fs::write(&path, rc4_permissions_only(&annotated_hello(scratch).await)).expect("write");
    open(path, None)
}

#[tokio::test]
async fn notes_on_a_document_that_needs_a_password_read_back_decoded() {
    let scratch = Scratch::new("notes");
    let handle = open_protected(&scratch, Some(PASSWORD), None).await;

    let notes = handle.read_notes().await.expect("read notes");
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert_eq!(notes[0].content, "Meet at noon");
    assert_eq!(notes[0].author, "Ana");
}

#[tokio::test]
async fn form_fields_on_a_document_that_needs_a_password_read_back_decoded() {
    let scratch = Scratch::new("fields");
    let handle = open_protected(&scratch, Some(PASSWORD), None).await;

    let fields = handle.read_text_fields(0).await.expect("read fields");
    let email = fields.iter().find(|f| f.name == "email").expect("the email field");
    assert_eq!(email.value, "ana@example.com");
}

// No open password, so the actor holds none and the decrypted copy is made
// with the empty one — the branch a document opened with a password never takes.
#[tokio::test]
async fn notes_on_a_permissions_only_document_read_back_decoded() {
    let scratch = Scratch::new("permissions-only");
    let handle = open_permissions_only(&scratch).await;

    let notes = handle.read_notes().await.expect("read notes");
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert_eq!(notes[0].content, "Meet at noon");
}

// The decrypted copy is for reading. Reading first must not change what a
// save writes.
#[tokio::test]
async fn reading_a_protected_document_never_saves_it_decrypted() {
    let scratch = Scratch::new("save");
    let handle = open_protected(&scratch, Some(PASSWORD), None).await;
    let _ = handle.read_notes().await.expect("read notes");
    let _ = handle.read_text_fields(0).await.expect("read fields");

    let dest = scratch.path("saved.pdf");
    handle.save(Some(dest.clone())).await.expect("save");
    assert!(declares_encryption(&std::fs::read(&dest).expect("read saved")), "saved without its protection");
    assert!(
        matches!(DocumentActorHandle::spawn(None, uuid::Uuid::new_v4(), dest, None).err(), Some(CommandError::PasswordRequired(_))),
        "the saved file opened without its password"
    );
}

// Inserting copies pages in with PDFium *before* the lopdf pass that refuses a
// protected document. When the refusal came only at the end, the pages had
// already been added to the live document, behind an edit reported as failed.
#[tokio::test]
async fn a_refused_insert_leaves_the_document_as_it_was() {
    let scratch = Scratch::new("insert");
    let handle = open_permissions_only(&scratch).await;
    let before = handle.page_count().await.expect("page count");

    let result = handle.insert_from_pdf(hello_path(), vec![0], 0).await;
    match result {
        Err(CommandError::InvalidInput(message)) => assert_eq!(message, PROTECTED_EDIT_REFUSAL),
        other => panic!("expected the protected-edit refusal, got {other:?}"),
    }
    // The actor caches its page count at open, so count what it would save.
    let dest = scratch.path("after-refused-insert.pdf");
    handle.save(Some(dest.clone())).await.expect("save");
    let after = open(dest, None).page_count().await.expect("page count");
    assert_eq!(after, before, "the refused insert added pages anyway");
}

#[tokio::test]
async fn merge_refuses_a_source_that_needs_a_password_by_name() {
    let scratch = Scratch::new("merge");
    let locked = scratch.path("locked.pdf");
    std::fs::write(&locked, protect(&std::fs::read(hello_path()).expect("hello"), Some(PASSWORD), None))
        .expect("write");
    let dest = scratch.path("merged.pdf");

    match merge_documents(&[hello_path(), locked], &dest) {
        Err(CommandError::InvalidInput(message)) => {
            assert!(message.starts_with("locked.pdf is password protected"), "{message}");
        }
        other => panic!("expected the source to be refused, got {other:?}"),
    }
    assert!(!dest.exists(), "a refused merge wrote a file");
}

// The actor path, which the background command's named check sits in front of:
// even without that check, lopdf must not import a page it cannot decrypt.
#[tokio::test]
async fn a_background_page_from_a_locked_pdf_is_refused() {
    let locked = protect(&std::fs::read(hello_path()).expect("hello"), Some(PASSWORD), None);
    let handle = open(hello_path(), None);

    let result = handle.add_background(vec![0], BackgroundKind::PdfPage { source: locked, page: 0 }, 1.0).await;
    match result {
        Err(CommandError::InvalidInput(message)) => assert_eq!(message, NEEDS_PASSWORD),
        other => panic!("expected the locked source to be refused, got {other:?}"),
    }
}
