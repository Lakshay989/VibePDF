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

// The verifier used to find the end of the signature by stripping trailing
// zeros from `/Contents`, which also stripped a final 0x00 that belongs to it —
// the last byte of the RSA signature, or of a timestamp token — so about one
// valid signature in 256 came back "not a readable PKCS#7 structure".
//
// Waiting for a real one takes ~256 seconds: the signature changes only when
// the CMS signing time ticks over. So this makes one. It adds an unsigned
// attribute — outside what the RSA signature covers, where a timestamp token
// goes — whose encoding ends `02 01 00`. The signature stays valid, and its
// DER now ends in 0x00.
#[test]
fn a_signature_whose_der_ends_in_zero_verifies() {
    use cms::content_info::ContentInfo;
    use cms::signed_data::{SignedData, SignerInfos};
    use der::asn1::SetOfVec;
    use der::{Any, Decode, Encode, Tag};
    use x509_cert::attr::Attribute;

    let mut doc = signed(None);
    let start = blob_hex_start(&doc);
    let close = start + doc[start..].iter().position(|b| *b == b'>').expect(">");
    let raw = unhex(&doc[start..close]);

    let info = ContentInfo::decode(&mut der::SliceReader::new(&raw).expect("reader")).expect("CMS");
    let original = info.to_der().expect("encode");
    assert_eq!(original, raw[..original.len()], "the CMS does not re-encode to itself");

    let mut signed_data: SignedData = info.content.decode_as().expect("SignedData");
    let mut signer = signed_data.signer_infos.0.get(0).expect("a signer").clone();
    let zero = Any::new(Tag::Integer, [0u8].as_slice()).expect("INTEGER 0");
    let attribute = Attribute {
        // An example arc (RFC 5612): it names nothing, so no reader acts on it.
        oid: const_oid::ObjectIdentifier::new_unwrap("1.3.6.1.4.1.32473.1"),
        values: SetOfVec::try_from(vec![zero]).expect("values"),
    };
    signer.unsigned_attrs = Some(SetOfVec::try_from(vec![attribute]).expect("attributes"));
    signed_data.signer_infos = SignerInfos(SetOfVec::try_from(vec![signer]).expect("signers"));
    let der = ContentInfo {
        content_type: info.content_type,
        content: Any::encode_from(&signed_data).expect("SignedData"),
    }
    .to_der()
    .expect("encode");
    assert_eq!(der.last(), Some(&0), "the rebuilt CMS should end in 0x00");

    let hex: String = der.iter().map(|b| format!("{b:02X}")).collect();
    assert!(hex.len() <= close - start, "the rebuilt CMS does not fit the gap");
    let padded = format!("{hex:0<width$}", width = close - start);
    doc[start..close].copy_from_slice(padded.as_bytes());

    let reports = verify_signatures(&doc, while_valid()).expect("verify");
    let r = &reports[0];
    assert!(r.is_intact(), "a valid signature ending in 0x00 was rejected: {r:?}");
    assert!(r.signature_valid && r.digest_matches);

    // The outside check: openssl, which shares no code with ours, accepts
    // the same blob over the same bytes — so the blob really is valid, and
    // the verifier is right to say so.
    let Some(openssl) = openssl_path() else {
        eprintln!("openssl not found; skipping the differential check");
        return;
    };
    let sig = TempFile::new("der", &der);
    let content = TempFile::new("bin", &signed_bytes(&doc));
    let out = std::process::Command::new(openssl)
        .args(["cms", "-verify", "-binary", "-inform", "DER", "-in"])
        .arg(&sig.0)
        .arg("-content")
        .arg(&content.0)
        .args(["-noverify", "-out", "/dev/null"])
        .output()
        .expect("run openssl");
    assert!(
        out.status.success(),
        "openssl rejected the zero-ending signature:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn unhex(hex: &[u8]) -> Vec<u8> {
    hex.chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).expect("hex"), 16).expect("hex digit"))
        .collect()
}

/// The bytes `/ByteRange` says were signed.
fn signed_bytes(doc: &[u8]) -> Vec<u8> {
    let at = find(doc, b"/ByteRange").expect("/ByteRange");
    let open = at + doc[at..].iter().position(|b| *b == b'[').expect("[");
    let close = open + doc[open..].iter().position(|b| *b == b']').expect("]");
    let n: Vec<usize> = String::from_utf8_lossy(&doc[open + 1..close])
        .split_whitespace()
        .map(|t| t.parse().expect("number"))
        .collect();
    [&doc[n[0]..n[0] + n[1]], &doc[n[2]..n[2] + n[3]]].concat()
}

fn openssl_path() -> Option<&'static str> {
    ["/opt/homebrew/bin/openssl", "/usr/local/bin/openssl", "openssl"]
        .into_iter()
        .find(|p| {
            std::process::Command::new(p)
                .arg("version")
                .output()
                .is_ok_and(|o| o.status.success())
        })
}

/// A temp file that removes itself.
struct TempFile(PathBuf);

impl TempFile {
    fn new(ext: &str, bytes: &[u8]) -> Self {
        let p = std::env::temp_dir().join(format!("vibepdf-verify-{}.{ext}", uuid::Uuid::new_v4()));
        std::fs::write(&p, bytes).expect("write temp");
        Self(p)
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
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
