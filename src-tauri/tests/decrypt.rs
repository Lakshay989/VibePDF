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

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use lopdf::encryption::crypt_filters::{Aes128CryptFilter, CryptFilter};
use lopdf::{Document, EncryptionState, EncryptionVersion, Object, Permissions, StringFormat};
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

// SPEC: P6-SEC-008 — "SHALL require the owner password". Until lopdf 0.42 this
// held for R6 only because lopdf's decrypt happened to refuse the user
// password; it is now checked by `remove_protection` itself, for every
// handler (see `every_handler_needs_the_owner_password` below).
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
//
// The fixture is git-ignored and built on demand (it needs pypdf, for the
// encryption), so a fresh clone and CI do not have it. This test asserted it
// existed from P6.C3 onwards, which failed every CI run that reached it from
// 2026-08-13 — and because cargo stops at the first failing test binary, it
// also hid every later binary's result. It now skips with the regenerate
// command, the convention `encrypted_open.rs` already follows.
#[test]
fn unlocks_a_document_from_another_tool() {
    let p = PathBuf::from("../tests/fixtures/acceptance/p1-encrypted.pdf");
    if !p.is_file() {
        eprintln!(
            "skipping: fixture missing at {}\n  regenerate with: \
             python3 tests/fixtures/acceptance/generate.py encrypted",
            p.display(),
        );
        return;
    }
    let bytes = std::fs::read(&p).expect("read");

    // `vibepdf` is this file's open password and `vibepdf-owner` its owner
    // password (generate.py). Unlocking needs the owner one.
    assert!(remove_protection(&bytes, "vibepdf").is_err(), "the open password must not remove protection");
    let out = remove_protection(&bytes, "vibepdf-owner").expect("remove");
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

/// `hello.pdf` encrypted by lopdf with an older handler — the kinds other tools
/// write — with open password `open-me` and owner password `owner-pw`.
/// VibePDF's own Protect writes only AES-256.
fn legacy_protected(handler: &str) -> Vec<u8> {
    let mut doc = Document::load_mem(&fixture_bytes("hello.pdf")).expect("load");
    let file_id = Object::String(b"0123456789abcdef".to_vec(), StringFormat::Hexadecimal);
    doc.trailer.set("ID", Object::Array(vec![file_id.clone(), file_id]));
    let version = match handler {
        "RC4-128" => EncryptionVersion::V2 {
            document: &doc,
            owner_password: "owner-pw",
            user_password: "open-me",
            key_length: 128,
            permissions: Permissions::all(),
        },
        "AES-128" => {
            let aes: Arc<dyn CryptFilter> = Arc::new(Aes128CryptFilter);
            EncryptionVersion::V4 {
                document: &doc,
                encrypt_metadata: true,
                crypt_filters: BTreeMap::from([(b"StdCF".to_vec(), aes)]),
                stream_filter: b"StdCF".to_vec(),
                string_filter: b"StdCF".to_vec(),
                owner_password: "owner-pw",
                user_password: "open-me",
                permissions: Permissions::all(),
            }
        }
        other => panic!("no such handler in this test: {other}"),
    };
    let state = EncryptionState::try_from(version).expect("encryption state");
    doc.encrypt(&state).expect("encrypt");
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("save");
    out
}

/// RC4 with an owner password and no open password: lopdf decrypts it on load.
fn permissions_only() -> Vec<u8> {
    let mut doc = Document::load_mem(&fixture_bytes("hello.pdf")).expect("load");
    let file_id = Object::String(b"0123456789abcdef".to_vec(), StringFormat::Hexadecimal);
    doc.trailer.set("ID", Object::Array(vec![file_id.clone(), file_id]));
    let state = EncryptionState::try_from(EncryptionVersion::V2 {
        document: &doc,
        owner_password: "owner-pw",
        user_password: "",
        key_length: 128,
        permissions: Permissions::all(),
    })
    .expect("RC4 state");
    doc.encrypt(&state).expect("encrypt");
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("save");
    out
}

fn refusal(result: Result<Vec<u8>, CommandError>) -> String {
    match result {
        Err(CommandError::InvalidInput(message)) => message,
        Ok(_) => panic!("protection was removed"),
        Err(other) => panic!("expected a refusal, got {other:?}"),
    }
}

// SPEC: P6-SEC-008 — "SHALL require the owner password", for every handler.
// On lopdf 0.36, RC4 and AES-128 files unlocked with the user password too.
#[test]
fn every_handler_needs_the_owner_password() {
    let aes256 = encrypt_document(
        &fixture_bytes("hello.pdf"),
        &EncryptOptions {
            user_password: Some("open-me".into()),
            owner_password: Some("owner-pw".into()),
            permissions: Default::default(),
        },
    )
    .expect("encrypt");

    for (handler, bytes) in [
        ("AES-256", aes256),
        ("RC4-128", legacy_protected("RC4-128")),
        ("AES-128", legacy_protected("AES-128")),
    ] {
        let user = refusal(remove_protection(&bytes, "open-me"));
        assert!(user.starts_with("That is the password to open the document"), "{handler}: {user}");
        let wrong = refusal(remove_protection(&bytes, "not-either"));
        assert_eq!(wrong, "That password did not unlock the document.", "{handler}");

        let out = remove_protection(&bytes, "owner-pw").unwrap_or_else(|e| panic!("{handler}: {e:?}"));
        let file = TempPdf::new(&out);
        let (doc, meta) =
            open_pdf(&file.0, None).unwrap_or_else(|e| panic!("{handler}: opens with no password: {e:?}"));
        assert_eq!(meta.page_count, 1, "{handler}");
        drop(doc);
    }
}

// lopdf opens a permissions-only file with the empty open password and throws
// away the encryption dictionary, so its owner password cannot be checked.
// Unlocking it anyway would accept any password; it is refused — with the
// right one too, which is a limitation and is worded as one.
#[test]
fn a_permissions_only_document_is_not_unlocked_by_any_password() {
    let bytes = permissions_only();
    for password in ["owner-pw", "anything", ""] {
        let message = refusal(remove_protection(&bytes, password));
        assert!(message.contains("aren't supported yet"), "{password:?}: {message}");
    }
}

#[test]
fn a_plain_document_is_not_rewritten() {
    let message = refusal(remove_protection(&fixture_bytes("hello.pdf"), "anything"));
    assert_eq!(message, "This PDF is not password protected.");
}
