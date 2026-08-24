//! The signature container (P6.B1a, part one of SPEC P6-SEC-005).
//!
//! No cryptography is exercised here — there is none in the module under test.
//! What these check is the arithmetic, which is the part that fails silently:
//! a `/ByteRange` that is wrong by one byte produces a file that opens
//! perfectly, shows a signature, and reports it as invalid, with nothing to say
//! which of the four numbers is off.
//!
//! The invariant every test circles is: **the signed bytes are the whole file
//! except the gap, and nothing moved after the offsets were written.**

use std::path::PathBuf;

use vibepdf_lib::pdf::document::open_pdf;
use vibepdf_lib::security::sign::{prepare, SignatureSpec, SignatureTarget};

fn fixture(name: &str) -> Vec<u8> {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
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
        certify: None,
        target: SignatureTarget::NewField,
    }
}

/// A temp file so PDFium can be pointed at the output.
struct TempPdf(PathBuf);

impl TempPdf {
    fn new(bytes: &[u8]) -> Self {
        let p = std::env::temp_dir().join(format!("vibepdf-sign-{}.pdf", uuid::Uuid::new_v4()));
        std::fs::write(&p, bytes).expect("write temp pdf");
        Self(p)
    }
}

impl Drop for TempPdf {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

// The defining property of an incremental update, and the reason signing uses
// one: a signature already on the document keeps covering the bytes it signed.
// If lopdf ever re-serialises instead of appending, this is what says so.
#[test]
fn the_original_bytes_are_untouched() {
    let src = fixture("hello.pdf");
    let prepared = prepare(&src, &spec()).expect("prepare");
    let out = prepared.embed(&[]).expect("embed nothing");

    assert!(out.len() > src.len(), "nothing was appended");
    assert_eq!(
        &out[..src.len()],
        &src[..],
        "the original revision was rewritten, not appended to"
    );
}

// SPEC: P6-SEC-005 — /ByteRange must describe the entire file apart from the
// gap. Four numbers, and every one of them is a chance to be off by one.
#[test]
fn the_byte_range_covers_everything_except_the_gap() {
    let src = fixture("hello.pdf");
    let prepared = prepare(&src, &spec()).expect("prepare");
    let [a_off, a_len, b_off, b_len] = prepared.byte_range();
    let message = prepared.message();
    let out = prepare(&src, &spec()).expect("prepare").embed(&[]).unwrap();

    assert_eq!(a_off, 0, "the first range must start at byte 0");
    assert_eq!(
        b_off + b_len,
        out.len(),
        "the second range must run to the end of the file"
    );
    assert!(a_len < b_off, "the ranges must not overlap the gap");
    assert_eq!(
        message.len(),
        a_len + b_len,
        "the signed message is exactly the two declared ranges"
    );
    assert_eq!(
        out.len() - message.len(),
        b_off - a_len,
        "the gap is everything the ranges leave out"
    );
}

// The gap holds the placeholder and nothing else — no stray byte of real
// document caught inside it, which would then be unsigned.
#[test]
fn the_gap_contains_only_the_placeholder() {
    let src = fixture("hello.pdf");
    let prepared = prepare(&src, &spec()).expect("prepare");
    let [_, a_len, b_off, _] = prepared.byte_range();
    let out = prepare(&src, &spec()).expect("prepare").embed(&[]).unwrap();

    let gap = &out[a_len..b_off];
    assert_eq!(gap[0], b'<', "the gap must open at the hex string");
    assert_eq!(gap[gap.len() - 1], b'>', "the gap must close at it too");
    assert!(
        gap[1..gap.len() - 1].iter().all(|b| b.is_ascii_hexdigit()),
        "something other than the placeholder is inside the gap"
    );
}

// The message is the file with the gap cut out — not the file with the gap
// zeroed, and not just the first range. Checked against the bytes rather than
// against the module's own arithmetic.
#[test]
fn the_message_is_the_file_minus_the_gap() {
    let src = fixture("hello.pdf");
    let prepared = prepare(&src, &spec()).expect("prepare");
    let [_, a_len, b_off, _] = prepared.byte_range();
    let message = prepared.message();
    let out = prepare(&src, &spec()).expect("prepare").embed(&[]).unwrap();

    let mut expected = Vec::new();
    expected.extend_from_slice(&out[..a_len]);
    expected.extend_from_slice(&out[b_off..]);
    assert_eq!(message, expected);
}

// Patching /ByteRange after hashing would invalidate the signature, so the
// patch must not move a byte. This is the one that catches a "helpful" reformat
// of the array.
#[test]
fn embedding_a_signature_does_not_move_anything() {
    let src = fixture("hello.pdf");
    let empty = prepare(&src, &spec()).expect("prepare").embed(&[]).unwrap();

    let prepared = prepare(&src, &spec()).expect("prepare");
    let range_before = prepared.byte_range();
    let message_before = prepared.message();
    let der = vec![0xAB; 2048];
    let signed = prepared.embed(&der).expect("embed");

    assert_eq!(signed.len(), empty.len(), "embedding changed the file length");

    // …and the signed bytes are still the same bytes.
    let [_, a_len, b_off, _] = range_before;
    let mut after = Vec::new();
    after.extend_from_slice(&signed[..a_len]);
    after.extend_from_slice(&signed[b_off..]);
    assert_eq!(
        message_before, after,
        "embedding changed a byte the signature covers"
    );

    // The blob really is in the gap, hex-encoded.
    let gap = &signed[a_len + 1..b_off - 1];
    assert_eq!(&gap[..4], b"ABAB");
    assert!(
        gap[der.len() * 2..].iter().all(|b| *b == b'0'),
        "the unused tail of the gap should stay zeros"
    );
}

#[test]
fn a_signature_too_large_for_the_gap_is_refused() {
    let src = fixture("hello.pdf");
    let prepared = prepare(&src, &spec()).expect("prepare");
    let too_big = vec![0u8; prepared.capacity() + 1];

    let err = prepared.embed(&too_big).unwrap_err();
    assert!(
        format!("{err:?}").contains("reserved"),
        "the error should say the gap is too small, got {err:?}"
    );
}

// The round-trip rule: a document with a reserved but empty signature is a
// real, openable state — a viewer shows an unsigned field.
#[test]
fn the_prepared_document_still_opens() {
    let src = fixture("hello.pdf");
    let out = prepare(&src, &spec()).expect("prepare").embed(&[]).unwrap();

    let file = TempPdf::new(&out);
    let (doc, meta) = open_pdf(&file.0, None).expect("PDFium opens the prepared file");
    assert_eq!(meta.page_count, 1);
    drop(doc);
}

// …and so does one with a signature in it, even though ours is nonsense here.
// PDFium parses the /Sig dictionary on open, so a malformed container shows up
// as a load failure rather than as a verification failure later.
#[test]
fn a_document_with_a_signature_in_it_still_opens() {
    let src = fixture("hello.pdf");
    let out = prepare(&src, &spec())
        .expect("prepare")
        .embed(&vec![0x30; 1024])
        .expect("embed");

    let file = TempPdf::new(&out);
    let (doc, meta) = open_pdf(&file.0, None).expect("PDFium opens the signed file");
    assert_eq!(meta.page_count, 1);
    drop(doc);
}

// Signing a document that already has a signature needs a second incremental
// update and leaving the first /ByteRange alone. Refused for now rather than
// silently patching the wrong array — which would break the existing signature
// and look like tampering.
#[test]
fn signing_twice_is_refused_rather_than_corrupting_the_first() {
    let src = fixture("hello.pdf");
    let once = prepare(&src, &spec()).expect("prepare").embed(&[]).unwrap();

    let err = prepare(&once, &spec()).unwrap_err();
    assert!(
        format!("{err:?}").contains("already-signed"),
        "expected a clear refusal, got {err:?}"
    );
}

// A document that already has a form must keep it: the signature field is added
// to /AcroForm /Fields, not substituted for what was there.
#[test]
fn an_existing_form_survives_signing() {
    let src = fixture("forms.pdf");
    let out = prepare(&src, &spec()).expect("prepare").embed(&[]).unwrap();

    let doc = lopdf::Document::load_mem(&out).expect("load");
    let root = doc
        .trailer
        .get(b"Root")
        .and_then(lopdf::Object::as_reference)
        .expect("/Root");
    let acro = doc
        .get_object(root)
        .and_then(lopdf::Object::as_dict)
        .expect("catalog")
        .get(b"AcroForm")
        .and_then(lopdf::Object::as_reference)
        .and_then(|r| doc.get_object(r))
        .and_then(lopdf::Object::as_dict)
        .expect("/AcroForm");

    let fields = acro
        .get(b"Fields")
        .and_then(lopdf::Object::as_array)
        .expect("/Fields");
    assert!(
        fields.len() > 1,
        "the original form fields were dropped, leaving {} field(s)",
        fields.len()
    );
    assert_eq!(
        acro.get(b"SigFlags").and_then(lopdf::Object::as_i64).ok(),
        Some(3),
        "/SigFlags must say the document has signatures and is append-only"
    );
}

/// SPEC: P6-SEC-005 — a file to inspect while the crypto half is still pending.
#[test]
#[ignore = "produces a verification artifact; run on demand"]
fn writes_verification_artifact() {
    let dir = PathBuf::from("../Sample PDFs");
    std::fs::create_dir_all(&dir).expect("ensure Sample PDFs dir");
    let out = prepare(&fixture("hello.pdf"), &spec())
        .expect("prepare")
        .embed(&[])
        .expect("embed");
    let path = dir.join("vibepdf-verify-sig-placeholder.pdf");
    std::fs::write(&path, &out).expect("write");
    eprintln!("wrote {}", path.display());
}
