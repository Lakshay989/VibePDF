//! Verifying signatures (P6.B2a, SPEC P6-SEC-006).
//!
//! A verifier that says "valid" for everything passes every happy-path test
//! that could be written, so nearly all of these are **negative**: they take a
//! document that really is signed, break one specific thing about it, and
//! require the report to notice *that thing* and not the others.
//!
//! The pairing that matters is digest-vs-signature. They fail for different
//! reasons — a changed document breaks the digest, a forged or swapped
//! signature breaks the signature — and a verifier that collapses them into one
//! boolean cannot tell a user which happened.

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use vibepdf_lib::security::sign::{sign_document, DocMdpLevel, SignatureSpec, SignatureTarget};
use vibepdf_lib::security::verify::{verify_signatures, ChainStatus};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(PathBuf::from("../tests/fixtures/basic").join(name)).expect("read fixture")
}

fn cert(name: &str) -> Vec<u8> {
    let p = PathBuf::from("../tests/fixtures/certs").join(name);
    assert!(
        p.is_file(),
        "fixture missing at {} — run tests/fixtures/certs/generate-test-cert.sh",
        p.display()
    );
    std::fs::read(p).expect("read fixture")
}

fn spec(certify: Option<DocMdpLevel>) -> SignatureSpec {
    SignatureSpec {
        field_name: "Signature1".into(),
        signed_at: "D:20260813104500+00'00'".into(),
        reason: Some("I approve this document".into()),
        location: Some("Manchester".into()),
        contact: None,
        name: Some("VibePDF Test Signer".into()),
        certify,
        target: SignatureTarget::NewField,
    }
}

fn signed(certify: Option<DocMdpLevel>) -> Vec<u8> {
    sign_document(&fixture("hello.pdf"), &spec(certify), &cert("signer.pfx"), "test123")
        .expect("sign")
}

/// A moment inside the test certificate's validity window.
fn while_valid() -> SystemTime {
    SystemTime::now()
}

// The happy path, stated once. Everything below breaks something.
#[test]
fn a_signature_we_made_verifies() {
    let reports = verify_signatures(&signed(None), while_valid()).expect("verify");
    assert_eq!(reports.len(), 1);

    let r = &reports[0];
    assert!(r.signature_valid, "signature rejected: {:?}", r.problems);
    assert!(r.digest_matches, "digest rejected: {:?}", r.problems);
    assert!(r.covers_whole_document);
    assert!(!r.certificate_expired);
    assert!(r.is_intact());
    assert!(r.signer.contains("VibePDF Test Signer"));
    assert_eq!(r.field_name.as_deref(), Some("Signature1"));
    assert_eq!(r.reason.as_deref(), Some("I approve this document"));
    assert!(r.problems.is_empty(), "{:?}", r.problems);
}

// Changing a byte of the document breaks the **digest** and leaves the
// signature itself intact — the signature is over the attributes, and those
// are untouched. A verifier that reports the signature as broken here is
// telling the user the wrong story about what happened.
#[test]
fn editing_the_document_breaks_the_digest_and_not_the_signature() {
    let mut doc = signed(None);
    // Somewhere in the first byte range, well clear of the signature itself.
    doc[400] ^= 0xff;

    let reports = verify_signatures(&doc, while_valid()).expect("verify");
    let r = &reports[0];
    assert!(!r.digest_matches, "a changed document still hashed the same");
    assert!(
        r.signature_valid,
        "the signature itself should still check out — only the document changed"
    );
    assert!(!r.is_intact());
}

// Altering a byte of the **embedded certificate** leaves the signature and the
// digest verifying perfectly — the signature commits to the signed attributes,
// not to the certificate carried beside them. What it breaks is the
// certificate's own signature, and a verifier that does not follow that link
// lets an attacker put any name they like next to a signature that checks out.
//
// This test found that gap rather than confirming it: before the chain check
// existed, everything here read as valid.
#[test]
fn altering_the_embedded_certificate_breaks_the_chain() {
    let mut doc = signed(None);
    // Byte 100 of the DER is inside the certificate, well before the signature.
    let at = blob_hex_start(&doc) + 200;
    doc[at] = flip(doc[at]);

    let reports = verify_signatures(&doc, while_valid()).expect("verify");
    let r = &reports[0];
    assert_eq!(
        r.chain,
        ChainStatus::Broken,
        "an altered certificate was not noticed"
    );
    assert!(!r.is_intact(), "an altered certificate was reported as intact");
    // …and the rest is genuinely still true, which is the point.
    assert!(r.digest_matches, "the document did not change");
}

// Corrupting the RSA signature value itself — the last field of the structure —
// breaks the signature while the document is untouched. The mirror image of the
// digest test: together they show the two checks are independent rather than
// one value reported twice.
#[test]
fn corrupting_the_signature_value_breaks_the_signature() {
    let mut doc = signed(None);
    let at = blob_hex_end(&doc) - 64;
    doc[at] = flip(doc[at]);

    let reports = verify_signatures(&doc, while_valid()).expect("verify");
    let r = &reports[0];
    assert!(!r.is_intact());
    assert!(
        !r.signature_valid || !r.problems.is_empty(),
        "a corrupted signature was accepted: {r:?}"
    );
}

// Appending to a signed file is how a second signature is added, so it is not
// automatically an attack — but it does mean the first signature no longer
// covers the whole document, and a reader should be told.
#[test]
fn appending_to_the_file_is_reported() {
    let mut doc = signed(None);
    doc.extend_from_slice(b"\n% something added later\n");

    let reports = verify_signatures(&doc, while_valid()).expect("verify");
    let r = &reports[0];
    assert!(
        !r.covers_whole_document,
        "bytes were appended and the report did not notice"
    );
    // The bytes it *does* cover are unchanged, so this stays true — that is the
    // distinction between "someone added a page" and "someone edited page 1".
    assert!(r.digest_matches);
}

// SPEC: P6-SEC-006 — "expired". `now` is a parameter precisely so this is
// testable without waiting a decade or shipping a stale certificate.
#[test]
fn an_expired_certificate_is_reported() {
    let doc = signed(None);
    let far_future = SystemTime::now() + Duration::from_secs(60 * 60 * 24 * 365 * 20);

    let reports = verify_signatures(&doc, far_future).expect("verify");
    let r = &reports[0];
    assert!(r.certificate_expired, "a 10-year certificate never expired");
    // Expiry does not retroactively break the mathematics.
    assert!(r.signature_valid && r.digest_matches);
}

// SPEC: P6-SEC-006 — "chain-trusted". We have no trust anchors, so the answer
// is never "trusted". This test exists to pin that: a self-signed certificate
// is reported as self-signed, not as a valid chain of one.
#[test]
fn a_self_signed_certificate_is_reported_as_self_signed() {
    let reports = verify_signatures(&signed(None), while_valid()).expect("verify");
    assert_eq!(reports[0].chain, ChainStatus::SelfSigned);
}

#[test]
fn a_certified_signature_reports_its_level() {
    let reports = verify_signatures(&signed(Some(DocMdpLevel::FormFilling)), while_valid())
        .expect("verify");
    assert_eq!(reports[0].certification_level, Some(2));

    let plain = verify_signatures(&signed(None), while_valid()).expect("verify");
    assert_eq!(plain[0].certification_level, None);
}

// Most documents are unsigned. That is not an error and not a finding.
#[test]
fn an_unsigned_document_reports_nothing() {
    let reports = verify_signatures(&fixture("hello.pdf"), while_valid()).expect("verify");
    assert!(reports.is_empty());
}

// A document with a reserved but empty signature field: the gap is there, the
// signature is not. It must be reported as a signature that does not check out,
// never skipped and never accepted.
#[test]
fn an_empty_signature_field_is_reported_rather_than_skipped() {
    use vibepdf_lib::security::sign::prepare;

    let doc = prepare(&fixture("hello.pdf"), &spec(None))
        .expect("prepare")
        .embed(&[])
        .expect("embed nothing");

    let reports = verify_signatures(&doc, while_valid()).expect("verify");
    assert_eq!(reports.len(), 1, "the empty field was not noticed");
    assert!(!reports[0].is_intact());
    assert!(
        !reports[0].problems.is_empty(),
        "an empty signature should say what is wrong with it"
    );
}

#[test]
fn a_legacy_certificate_verifies_too() {
    let doc = sign_document(
        &fixture("hello.pdf"),
        &spec(None),
        &cert("signer-legacy.pfx"),
        "test123",
    )
    .expect("sign");
    let reports = verify_signatures(&doc, while_valid()).expect("verify");
    assert!(reports[0].is_intact());
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// First hex digit of the signature blob. Located by `/Contents<` rather than
/// `/Contents `, because pages have a `/Contents` too and lopdf writes the
/// signature's without a space.
fn blob_hex_start(doc: &[u8]) -> usize {
    find(doc, b"/Contents<").expect("the signature's /Contents") + 10
}

/// One past the last *non-padding* hex digit — the end of the real DER.
fn blob_hex_end(doc: &[u8]) -> usize {
    let start = blob_hex_start(doc);
    let close = start + doc[start..].iter().position(|b| *b == b'>').expect(">");
    let last = doc[start..close]
        .iter()
        .rposition(|b| *b != b'0')
        .expect("some signature");
    start + last + 1
}

fn flip(b: u8) -> u8 {
    if b == b'A' {
        b'B'
    } else {
        b'A'
    }
}
