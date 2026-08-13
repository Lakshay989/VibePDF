//! End-to-end certificate signing (P6.B1a, SPEC P6-SEC-005).
//!
//! **The load-bearing test here is `openssl_verifies_our_signature`.** Every
//! other assertion checks that our own code agrees with itself, which is worth
//! little for cryptography: a signature computed over the wrong bytes with a
//! consistently wrong verifier passes every self-consistent test there is. So
//! the signature is handed to an implementation that has never seen this
//! codebase — the same differential check that found the `/Perms` bug in P6.C1,
//! where PDFium disagreed with our tests and PDFium was right.

use std::path::PathBuf;
use std::process::Command;

use vibepdf_lib::pdf::document::open_pdf;
use vibepdf_lib::security::sign::{sign_document, SignatureSpec};

fn fixture(name: &str) -> Vec<u8> {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    std::fs::read(p).expect("read fixture")
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

fn spec() -> SignatureSpec {
    SignatureSpec {
        field_name: "Signature1".into(),
        signed_at: "D:20260813104500+00'00'".into(),
        reason: Some("I approve this document".into()),
        location: Some("Manchester".into()),
        contact: None,
        name: Some("VibePDF Test Signer".into()),
    }
}

fn sign(pfx: &str) -> Vec<u8> {
    sign_document(&fixture("hello.pdf"), &spec(), &cert(pfx), "test123").expect("sign")
}

/// The `/Contents` blob and the bytes it covers, pulled back out of the file
/// the way a verifier would — by reading `/ByteRange`, not by trusting us.
fn extract(signed: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let at = find(signed, b"/ByteRange").expect("/ByteRange");
    let open = at + signed[at..].iter().position(|b| *b == b'[').expect("[");
    let close = open + signed[open..].iter().position(|b| *b == b']').expect("]");
    let nums: Vec<usize> = String::from_utf8_lossy(&signed[open + 1..close])
        .split_whitespace()
        .map(|t| t.parse().expect("a number"))
        .collect();
    assert_eq!(nums.len(), 4, "/ByteRange must have four numbers");

    let mut message = Vec::new();
    message.extend_from_slice(&signed[nums[0]..nums[0] + nums[1]]);
    message.extend_from_slice(&signed[nums[2]..nums[2] + nums[3]]);

    // The gap between the ranges is `<hex>`; strip the delimiters and the
    // zero padding to get the DER.
    let gap = &signed[nums[0] + nums[1]..nums[2]];
    let hex = &gap[1..gap.len() - 1];
    let mut der = Vec::new();
    for pair in hex.chunks(2) {
        let s = std::str::from_utf8(pair).expect("hex");
        der.push(u8::from_str_radix(s, 16).expect("hex byte"));
    }
    while der.last() == Some(&0) {
        der.pop();
    }
    (der, message)
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

struct Temp(PathBuf);
impl Temp {
    fn new(ext: &str, bytes: &[u8]) -> Self {
        let p = std::env::temp_dir()
            .join(format!("vibepdf-pades-{}.{ext}", uuid::Uuid::new_v4()));
        std::fs::write(&p, bytes).expect("write temp");
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

// SPEC: P6-SEC-005 — the one test that could catch us signing the wrong bytes.
//
// `openssl cms -verify` recomputes the digest over the content we hand it,
// checks it against the signed message-digest attribute, and checks the
// signature over the attributes with the embedded certificate. Nothing in this
// repo is involved in that judgement.
//
// `-noverify` skips *chain* trust only: the certificate is self-signed, so
// there is no chain to trust. The signature itself is fully checked.
#[test]
fn openssl_verifies_our_signature() {
    let Some(openssl) = openssl_path() else {
        eprintln!("openssl not found; skipping the differential check");
        return;
    };
    let signed = sign("signer.pfx");
    let (der, message) = extract(&signed);

    let sig = Temp::new("der", &der);
    let content = Temp::new("bin", &message);

    let out = Command::new(openssl)
        .args(["cms", "-verify", "-binary", "-inform", "DER", "-in"])
        .arg(&sig.0)
        .arg("-content")
        .arg(&content.0)
        .args(["-noverify", "-out", "/dev/null"])
        .output()
        .expect("run openssl");

    assert!(
        out.status.success(),
        "openssl rejected our signature:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// The counter-test. If `openssl_verifies_our_signature` passed because the
// command silently succeeds on anything, this fails too and both are worthless.
#[test]
fn openssl_rejects_a_tampered_document() {
    let Some(openssl) = openssl_path() else {
        eprintln!("openssl not found; skipping the differential check");
        return;
    };
    let signed = sign("signer.pfx");
    let (der, mut message) = extract(&signed);

    // Change one byte of the signed content.
    let at = message.len() / 2;
    message[at] ^= 0xff;

    let sig = Temp::new("der", &der);
    let content = Temp::new("bin", &message);

    let out = Command::new(openssl)
        .args(["cms", "-verify", "-binary", "-inform", "DER", "-in"])
        .arg(&sig.0)
        .arg("-content")
        .arg(&content.0)
        .args(["-noverify", "-out", "/dev/null"])
        .output()
        .expect("run openssl");

    assert!(
        !out.status.success(),
        "openssl accepted a document with a flipped byte — the check proves nothing"
    );
}

// Both certificate flavours produce a signature, because the .pfx reader is the
// only thing that differs between them.
#[test]
fn a_legacy_certificate_signs_too() {
    let signed = sign("signer-legacy.pfx");
    let (der, _) = extract(&signed);
    assert!(!der.is_empty(), "no signature was written");
}

// SPEC: P6-SEC-005 — "embed the certificate chain".
#[test]
fn the_certificate_travels_with_the_signature() {
    let signed = sign("signer.pfx");
    let (der, _) = extract(&signed);

    // The signer's DN appears inside the CMS blob because the certificate is
    // embedded there; a verifier with no copy of it can still build a path.
    assert!(
        find(&der, b"VibePDF Test Signer").is_some(),
        "the signing certificate was not embedded"
    );
}

// PAdES, not the older adbe.pkcs7.detached — and the claim has to match what
// the blob actually is.
#[test]
fn the_signature_declares_the_pades_subfilter() {
    let signed = sign("signer.pfx");
    assert!(find(&signed, b"/ETSI.CAdES.detached").is_some());
    assert!(find(&signed, b"/Adobe.PPKLite").is_some());
}

// The round-trip rule.
#[test]
fn a_signed_document_still_opens() {
    let signed = sign("signer.pfx");
    let file = Temp::new("pdf", &signed);
    let (doc, meta) = open_pdf(&file.0, None).expect("PDFium opens the signed file");
    assert_eq!(meta.page_count, 1);
    drop(doc);
}

#[test]
fn a_wrong_certificate_password_fails_before_anything_is_written() {
    let err = sign_document(
        &fixture("hello.pdf"),
        &spec(),
        &cert("signer.pfx"),
        "wrong",
    )
    .unwrap_err();
    assert!(format!("{err:?}").contains("password"));
}

fn openssl_path() -> Option<&'static str> {
    ["/opt/homebrew/bin/openssl", "/usr/local/bin/openssl", "openssl"]
        .into_iter()
        .find(|p| {
            Command::new(p)
                .arg("version")
                .output()
                .is_ok_and(|o| o.status.success())
        })
}

/// SPEC: P6-SEC-005 — a signed file to open in Acrobat.
#[test]
#[ignore = "produces a verification artifact; run on demand"]
fn writes_verification_artifact() {
    let dir = PathBuf::from("../Sample PDFs");
    std::fs::create_dir_all(&dir).expect("ensure Sample PDFs dir");
    let path = dir.join("vibepdf-verify-signed.pdf");
    std::fs::write(&path, sign("signer.pfx")).expect("write");
    eprintln!("wrote {}", path.display());
}
