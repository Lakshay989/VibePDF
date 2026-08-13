//! Integration tests for removing password protection (P6.C2).
//!
//! SPEC: P6-SEC-008 — "SHALL require the owner password and SHALL re-save the
//! PDF without encryption."
//!
//! The property that matters is a round trip: whatever P6.C1 protects, this
//! must unlock, and the result must be genuinely unprotected rather than
//! merely reported as such. "Still encrypted" and "unlocked" look identical
//! from the outside until someone tries to open the file somewhere else, so
//! PDFium's opinion is what these assert against.

use std::path::PathBuf;

use lopdf::{Document, Object};
use vibepdf_lib::error::CommandError;
use vibepdf_lib::pdf::document::open_pdf;
use vibepdf_lib::security::decrypt::remove_protection;
use vibepdf_lib::security::encrypt::{encrypt_document, EncryptOptions};

fn fixture_bytes(name: &str) -> Vec<u8> {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    std::fs::read(p).expect("read fixture")
}

struct TempPdf(PathBuf);

impl TempPdf {
    fn new(bytes: &[u8]) -> Self {
        let p = std::env::temp_dir().join(format!("vibepdf-dec-{}.pdf", uuid::Uuid::new_v4()));
        std::fs::write(&p, bytes).expect("write temp pdf");
        Self(p)
    }
}

impl Drop for TempPdf {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn protected(user: &str) -> Vec<u8> {
    encrypt_document(
        &fixture_bytes("hello.pdf"),
        &EncryptOptions {
            user_password: Some(user.to_string()),
            owner_password: None,
            permissions: Default::default(),
        },
    )
    .expect("encrypt")
}

// SPEC: P6-SEC-008 — the round trip through P6.C1.
#[test]
fn unlocks_what_we_protected() {
    let out = remove_protection(&protected("open-me"), "open-me").expect("remove");

    let doc = Document::load_mem(&out).expect("load");
    assert!(!doc.is_encrypted(), "/Encrypt survived");
    assert!(
        doc.trailer.get(b"Encrypt").is_err(),
        "the trailer still points at an encryption dictionary"
    );

    // The only opinion that counts: a reader opens it with no password at all.
    let file = TempPdf::new(&out);
    let (doc, meta) = open_pdf(&file.0, None).expect("opens with no password");
    assert_eq!(meta.page_count, 1);
    drop(doc);
}

// SPEC: P6-SEC-008 — unlocking must not cost the document.
#[test]
fn the_content_survives() {
    let out = remove_protection(&protected("pw"), "pw").expect("remove");
    let file = TempPdf::new(&out);

    let (doc, _) = open_pdf(&file.0, None).expect("open");
    let runs = vibepdf_lib::pdf::text_extract::extract_text_runs(&doc, 0).expect("extract");
    let text: String = runs.iter().map(|r| r.text.as_str()).collect();
    assert!(text.contains("Hello"), "page text should survive, got {text:?}");
    drop(doc);
}

// SPEC: P6-SEC-008 — "SHALL require the owner password". For R6 that is
// enforced by lopdf rather than by us: it authenticates the owner password and
// rejects the user one. Pinned because it is surprising, and because a future
// lopdf that accepts both would quietly widen what this command allows.
#[test]
fn requires_the_owner_password_not_the_user_one() {
    let bytes = encrypt_document(
        &fixture_bytes("hello.pdf"),
        &EncryptOptions {
            user_password: Some("open-me".into()),
            owner_password: Some("owner-only".into()),
            permissions: Default::default(),
        },
    )
    .expect("encrypt");

    let out = remove_protection(&bytes, "owner-only").expect("the owner password unlocks it");
    assert!(!Document::load_mem(&out).expect("load").is_encrypted());

    assert!(
        remove_protection(&bytes, "open-me").is_err(),
        "the user password must not remove protection"
    );
}

// SPEC: P6-SEC-008 — the RC4-128 fixture, i.e. a file this project did not
// write. Protection removal has to work on other people's documents.
#[test]
fn unlocks_a_document_from_another_tool() {
    let p = PathBuf::from("../tests/fixtures/acceptance/p1-encrypted.pdf");
    assert!(p.is_file(), "fixture missing");
    let bytes = std::fs::read(&p).expect("read");

    let out = remove_protection(&bytes, "vibepdf").expect("remove");
    let file = TempPdf::new(&out);
    let (doc, _) = open_pdf(&file.0, None).expect("opens with no password");
    drop(doc);
}

// The AES-256 variant lopdf cannot open must say so, not read as a bad
// password. `InvalidKeyLength` sent through the generic path would have the
// user retyping a password that was correct all along.
#[test]
fn explains_the_variant_it_cannot_unlock() {
    // A V5 document carrying /Length — what pypdf writes, and what we briefly
    // wrote ourselves.
    let mut doc = Document::load_mem(&protected("pw")).expect("load");
    let id = doc
        .trailer
        .get(b"Encrypt")
        .and_then(Object::as_reference)
        .expect("/Encrypt");
    doc.get_object_mut(id)
        .and_then(Object::as_dict_mut)
        .expect("dict")
        .set("Length", 256_i64);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("save");

    let err = remove_protection(&bytes, "pw").unwrap_err();
    let CommandError::InvalidInput(msg) = &err else { panic!("got {err:?}") };
    assert!(
        msg.contains("AES-256"),
        "should name the variant rather than blame the password: {msg}"
    );
}
