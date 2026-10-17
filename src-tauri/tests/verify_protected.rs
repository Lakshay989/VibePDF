//! SPEC: P6-SEC-006 — "WHEN the user opens a signed PDF, THE system SHALL
//! verify all signatures", including when the PDF is password protected.
//!
//! The fixtures are signed *after* being protected, as Acrobat does it, and
//! both steps were done by pyHanko — which also validates them: intact, valid,
//! trusted against the committed test certificate, covering the whole file
//! (`tests/fixtures/basic/generate-signed-protected.py`). VibePDF's verifier is
//! checked against that outside verdict, not against its own signer.
//!
//! Two failures this guards. lopdf parses a file that needs a password as
//! nothing but its encryption dictionary, so without the password the
//! signature was not found at all — the document read as unsigned. And with
//! the password, lopdf "decrypts" the signature's `/Contents` too, although the
//! standard never encrypts it: harmless for AES at these lengths, garbage for
//! RC4. Hence one fixture per cipher.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use vibepdf_lib::error::CommandError;
use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::document::NEEDS_PASSWORD;
use vibepdf_lib::security::verify::{verify_signatures, SignatureReport};

const OPEN_PASSWORD: &str = "open-me";
const FIXTURES: [&str; 2] = ["signed-protected.pdf", "signed-protected-rc4.pdf"];

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/basic").join(name)
}

async fn verify_as_the_app_does(name: &str) -> Vec<SignatureReport> {
    let path = fixture(name);
    let file = std::fs::read(&path).expect("fixture");
    let handle =
        DocumentActorHandle::spawn(None, uuid::Uuid::new_v4(), path, Some(OPEN_PASSWORD.into())).expect("opens");
    handle.verify_signatures(file, SystemTime::now()).await.expect("verify")
}

#[tokio::test]
async fn a_signed_protected_document_verifies_with_its_password() {
    for name in FIXTURES {
        let reports = verify_as_the_app_does(name).await;
        assert_eq!(reports.len(), 1, "{name}: {reports:?}");
        let report = &reports[0];
        assert!(report.problems.is_empty(), "{name}: {:?}", report.problems);
        assert!(report.digest_matches, "{name}: the signed bytes no longer match");
        assert!(report.signature_valid, "{name}: the signature does not verify");
        assert!(report.covers_whole_document, "{name}: pyHanko says it covers the whole file");
        assert!(report.signer.contains("VibePDF Test Signer"), "{name}: signer {:?}", report.signer);
        assert_eq!(report.field_name.as_deref(), Some("Approval"), "{name}");
    }
}

// Without the password lopdf finds no signatures, and an empty list would read
// as "unsigned". It must be an error that says why instead.
#[test]
fn without_the_password_it_is_an_error_not_an_unsigned_document() {
    for name in FIXTURES {
        let file = std::fs::read(fixture(name)).expect("fixture");
        match verify_signatures(&file, SystemTime::now()) {
            Err(CommandError::InvalidInput(message)) => assert_eq!(message, NEEDS_PASSWORD, "{name}"),
            other => panic!("{name}: expected the needs-password refusal, got {other:?}"),
        }
    }
}

// A protected file whose signed bytes changed must still fail — reading
// `/Contents` from the file must not turn into reading past the check.
#[tokio::test]
async fn tampering_with_a_signed_protected_document_is_still_caught() {
    for name in FIXTURES {
        let path = fixture(name);
        let mut file = std::fs::read(&path).expect("fixture");
        // The `%PDF-` header's version digit sits inside the first signed range.
        file[7] = if file[7] == b'7' { b'6' } else { b'7' };
        let handle =
            DocumentActorHandle::spawn(None, uuid::Uuid::new_v4(), path, Some(OPEN_PASSWORD.into())).expect("opens");
        let reports = handle.verify_signatures(file, SystemTime::now()).await.expect("verify");
        assert_eq!(reports.len(), 1, "{name}");
        assert!(!reports[0].digest_matches, "{name}: a changed signed byte went unnoticed");
    }
}
