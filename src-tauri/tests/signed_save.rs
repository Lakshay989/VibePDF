//! SPEC: P6-SEC-006 — saving an edit to a signed document keeps its signature.
//!
//! Found by the P6 sweep: add a note to a signed file, save, undo, save again,
//! and the banner said "The signature covers bytes outside the file". Every
//! save rewrote the whole file, moving the bytes the signature covers. Now a
//! signed document's saves are appended as incremental updates.
//!
//! The assertions are about data. The load-bearing one is that the saved file
//! *begins with every byte that was signed* — if that holds, the signature
//! cannot have been disturbed, whatever any verifier says. The verifier and
//! OpenSSL then confirm it independently, and the edit is checked to be really
//! in the file, because an incremental save that forgot the edit would pass
//! every signature assertion here.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::security::encrypt::{encrypt_document, DocumentPermissions, EncryptOptions};
use vibepdf_lib::security::sign::{sign_document, DocMdpLevel, SignatureSpec, SignatureTarget};
use vibepdf_lib::security::verify::{verify_signatures, SignatureReport};

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("vibepdf-signed-save-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Self(dir)
    }

    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, bytes).expect("write");
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture(rel: &str) -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures").join(rel))
        .expect("fixture")
}

fn signed_hello(certify: Option<DocMdpLevel>) -> Vec<u8> {
    let spec = SignatureSpec {
        field_name: "Signature1".into(),
        signed_at: "D:20260813104500+00'00'".into(),
        reason: Some("I approve this document".into()),
        location: None,
        contact: None,
        name: Some("VibePDF Test Signer".into()),
        certify,
        target: SignatureTarget::NewField,
    };
    sign_document(&fixture("basic/hello.pdf"), &spec, &fixture("certs/signer.pfx"), "test123").expect("sign")
}

fn open(path: PathBuf, password: Option<&str>) -> DocumentActorHandle {
    DocumentActorHandle::spawn(None, uuid::Uuid::new_v4(), path, password.map(str::to_string)).expect("opens")
}

async fn add_note(handle: &DocumentActorHandle) {
    handle
        .add_note(uuid::Uuid::new_v4().to_string(), 0, 100.0, 600.0, "Checked".into(), "Reviewer".into())
        .await
        .expect("add note");
}

fn only_signature(bytes: &[u8]) -> SignatureReport {
    let mut reports = verify_signatures(bytes, SystemTime::now()).expect("verify");
    assert_eq!(reports.len(), 1, "expected exactly one signature");
    reports.remove(0)
}

/// Notes on page 1 of the document's *current* revision — through the page's
/// `/Annots`, so an object left behind in an earlier revision is not counted.
fn notes_on_first_page(bytes: &[u8]) -> usize {
    let doc = lopdf::Document::load_mem(bytes).expect("load");
    let page = *doc.get_pages().values().next().expect("a page");
    let Ok(annots) = doc.get_dictionary(page).expect("page").get(b"Annots") else {
        return 0;
    };
    let annots = match annots {
        lopdf::Object::Reference(id) => doc.get_object(*id).and_then(lopdf::Object::as_array).expect("annots"),
        other => other.as_array().expect("annots"),
    };
    annots
        .iter()
        .filter_map(|a| a.as_reference().ok().and_then(|id| doc.get_dictionary(id).ok()))
        .filter(|d| d.get(b"Subtype").and_then(lopdf::Object::as_name).is_ok_and(|s| s == b"Text"))
        .count()
}

fn eof_markers(bytes: &[u8]) -> usize {
    bytes.windows(5).filter(|w| w == b"%%EOF").count()
}

/// The part that matters, asserted on the bytes before any verifier is asked.
fn assert_signed_revision_untouched(signed: &[u8], saved: &[u8]) {
    assert!(saved.starts_with(signed), "every signed byte must still be exactly where it was");
    let report = only_signature(saved);
    assert!(report.problems.is_empty(), "verifier problems: {:?}", report.problems);
    assert!(report.signature_valid, "the signature must still check out");
    assert!(report.digest_matches, "the signed bytes must still hash to what was signed");
}

#[tokio::test]
async fn saving_an_edit_keeps_the_signature_valid() {
    let scratch = Scratch::new("edit");
    let signed = signed_hello(None);
    let path = scratch.write("signed.pdf", &signed);

    let handle = open(path.clone(), None);
    add_note(&handle).await;
    handle.save(None).await.expect("save");

    let saved = std::fs::read(&path).expect("read saved");
    assert_signed_revision_untouched(&signed, &saved);
    assert!(
        !only_signature(&saved).covers_whole_document,
        "a reader must be told the file changed after it was signed"
    );
    assert_eq!(notes_on_first_page(&saved), 1, "the edit itself must be in the saved file");
    assert_eq!(open(path, None).metadata().page_count, 1, "our own engine must still open it");
}

// The exact sequence from the sweep.
#[tokio::test]
async fn add_save_undo_save_keeps_the_signature_valid() {
    let scratch = Scratch::new("undo");
    let signed = signed_hello(None);
    let path = scratch.write("signed.pdf", &signed);

    let handle = open(path.clone(), None);
    add_note(&handle).await;
    handle.save(None).await.expect("first save");
    handle.undo().await.expect("undo");
    handle.save(None).await.expect("second save");

    let saved = std::fs::read(&path).expect("read saved");
    assert_signed_revision_untouched(&signed, &saved);
    assert_eq!(notes_on_first_page(&saved), 0, "the undone note must be gone from the current revision");
}

#[tokio::test]
async fn repeated_saves_keep_appending_to_the_signed_bytes() {
    let scratch = Scratch::new("repeat");
    let signed = signed_hello(None);
    let path = scratch.write("signed.pdf", &signed);

    let handle = open(path.clone(), None);
    add_note(&handle).await;
    handle.save(None).await.expect("first save");
    let after_first = std::fs::read(&path).expect("read");
    add_note(&handle).await;
    handle.save(None).await.expect("second save");

    let saved = std::fs::read(&path).expect("read saved");
    assert_signed_revision_untouched(&signed, &saved);
    assert!(saved.starts_with(&after_first), "the second save must append to the first, not replace it");
    assert_eq!(notes_on_first_page(&saved), 2);
    assert_eq!(eof_markers(&saved), eof_markers(&signed) + 2, "one appended revision per save");
}

// Our verifier agreeing with our saver is worth little on its own.
#[tokio::test]
async fn openssl_still_verifies_the_signed_revision_after_saving() {
    let Some(openssl) = openssl_path() else {
        eprintln!("openssl not found; skipping the differential check");
        return;
    };
    let scratch = Scratch::new("openssl");
    let path = scratch.write("signed.pdf", &signed_hello(None));
    let handle = open(path.clone(), None);
    add_note(&handle).await;
    handle.save(None).await.expect("save");

    let (der, message) = extract(&std::fs::read(&path).expect("read"));
    let sig = scratch.write("sig.der", &der);
    let content = scratch.write("content.bin", &message);
    let out = Command::new(openssl)
        .args(["cms", "-verify", "-binary", "-inform", "DER", "-noverify", "-in"])
        .arg(&sig)
        .arg("-content")
        .arg(&content)
        .args(["-out", "/dev/null"])
        .output()
        .expect("run openssl");
    assert!(out.status.success(), "openssl rejected it: {}", String::from_utf8_lossy(&out.stderr));
}

#[tokio::test]
async fn a_certified_document_keeps_its_certification_in_the_signed_revision() {
    let scratch = Scratch::new("certified");
    let signed = signed_hello(Some(DocMdpLevel::NoChanges));
    let path = scratch.write("certified.pdf", &signed);
    let handle = open(path.clone(), None);
    add_note(&handle).await;
    handle.save(None).await.expect("save");

    let saved = std::fs::read(&path).expect("read saved");
    assert_signed_revision_untouched(&signed, &saved);
    // Whether a later change was *allowed* is the reader's judgement to make
    // against the certification level; ours is not to erase the evidence.
    assert_eq!(only_signature(&saved).certification_level, Some(1));
}

#[tokio::test]
async fn an_unsigned_document_is_still_saved_whole() {
    let scratch = Scratch::new("unsigned");
    let path = scratch.write("hello.pdf", &fixture("basic/hello.pdf"));
    let handle = open(path.clone(), None);
    add_note(&handle).await;
    handle.save(None).await.expect("save");

    let saved = std::fs::read(&path).expect("read saved");
    assert_eq!(eof_markers(&saved), 1, "unsigned documents keep a single revision");
    assert_eq!(notes_on_first_page(&saved), 1);
}

// Appending to an encrypted file needs each object encrypted with the document
// key. Refusing keeps the edit in the open document and the file intact;
// rewriting would break the signature, which is what this suite exists to stop.
#[tokio::test]
async fn a_signed_and_protected_document_refuses_instead_of_breaking() {
    let scratch = Scratch::new("protected");
    let opts = EncryptOptions {
        user_password: Some("open-me".into()),
        owner_password: None,
        permissions: DocumentPermissions::default(),
    };
    let protected = encrypt_document(&signed_hello(None), &opts).expect("encrypt");
    let path = scratch.write("protected.pdf", &protected);

    let handle = open(path.clone(), Some("open-me"));
    // Rotation, not a note: adding a note to a password-opened document fails
    // on its own (`PasswordRequired`) — a separate, pre-existing defect that
    // would stop this test short of the branch it exists to reach.
    handle.rotate_pages(vec![0], 1).await.expect("rotate");
    let err = handle.save(None).await.expect_err("saving must refuse");
    assert!(format!("{err:?}").contains("signed and password protected"), "{err:?}");
    assert_eq!(std::fs::read(&path).expect("read"), protected, "the file on disk must be untouched");
}

// The same refusal for protection lopdf *can* decrypt — RC4 with no open
// password, as other tools write it. lopdf opens that kind quietly and drops
// its encryption dictionary, so a check made after parsing saw an unprotected
// file; the save then failed its own read-back check with "object 5 0 did not
// survive saving". The check is made on the bytes, before parsing.
#[tokio::test]
async fn a_signed_permissions_only_document_refuses_too() {
    let scratch = Scratch::new("permissions-only");
    let protected = rc4_permissions_only(&signed_hello(None));
    let path = scratch.write("permissions-only.pdf", &protected);

    let handle = open(path.clone(), None);
    handle.rotate_pages(vec![0], 1).await.expect("rotate");
    let err = handle.save(None).await.expect_err("saving must refuse");
    assert!(format!("{err:?}").contains("signed and password protected"), "{err:?}");
    assert_eq!(std::fs::read(&path).expect("read"), protected, "the file on disk must be untouched");
}

/// RC4, an owner password and no open password, built by lopdf.
fn rc4_permissions_only(bytes: &[u8]) -> Vec<u8> {
    use lopdf::{EncryptionState, EncryptionVersion, Object, Permissions, StringFormat};
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
    out
}

fn extract(signed: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let at = signed.windows(10).position(|w| w == b"/ByteRange").expect("/ByteRange");
    let open = at + signed[at..].iter().position(|b| *b == b'[').expect("[");
    let close = open + signed[open..].iter().position(|b| *b == b']').expect("]");
    let n: Vec<usize> = String::from_utf8_lossy(&signed[open + 1..close])
        .split_whitespace()
        .map(|t| t.parse().expect("number"))
        .collect();
    let mut message = signed[n[0]..n[0] + n[1]].to_vec();
    message.extend_from_slice(&signed[n[2]..n[2] + n[3]]);
    let hex = &signed[n[0] + n[1] + 1..n[2] - 1];
    let mut der: Vec<u8> = hex
        .chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).expect("hex"), 16).expect("hex digit"))
        .collect();
    let len = der_len(&der);
    assert!(len <= der.len(), "the DER claims {len} bytes; the gap holds {}", der.len());
    der.truncate(len);
    (der, message)
}

/// The length of the DER value at the start of `der`, read from its own
/// header: the SEQUENCE tag, then a short- or long-form length.
///
/// Not "strip the trailing zeros". The padding is zeros, but so is about one
/// signature's final byte in 256 — that byte is the RSA signature's last — and
/// stripping it hands openssl a truncated blob ("not enough data"). CI failed
/// on exactly that in `sign_pades` on 2026-10-18.
fn der_len(der: &[u8]) -> usize {
    assert_eq!(der[0], 0x30, "/Contents does not start with a DER SEQUENCE");
    let first = usize::from(der[1]);
    if first < 0x80 {
        return 2 + first;
    }
    let n = first & 0x7f;
    let len = der[2..2 + n].iter().fold(0, |acc, b| (acc << 8) | usize::from(*b));
    2 + n + len
}

// The regression test for `der_len`: a blob whose own final byte is 0x00,
// followed by the placeholder's padding, must come back whole. Real signatures
// end in 0x00 too rarely (and only by the clock) to rely on one turning up.
#[test]
fn extract_keeps_a_signature_that_ends_in_zero() {
    // A SEQUENCE with a long-form length (0x0104 = 260 content bytes), the
    // last of them 0x00, then zero padding.
    let mut der = vec![0x30, 0x82, 0x01, 0x04];
    der.extend([0xAB; 259]);
    der.push(0x00);
    let hex: String = der.iter().chain(&[0u8; 64]).map(|b| format!("{b:02x}")).collect();
    let gap = format!("<{hex}>");
    let tail = "%%EOF";
    // Fixed-width numbers, so the header's length doesn't depend on them.
    let head_len = "/ByteRange [0 0000000000 0000000000 0000000000] ".len();
    let head = format!(
        "/ByteRange [0 {head_len:010} {:010} {:010}] ",
        head_len + gap.len(),
        tail.len()
    );
    let (got, message) = extract(format!("{head}{gap}{tail}").as_bytes());
    assert_eq!(got, der, "the final 0x00 is the signature's, not padding");
    assert_eq!(message, format!("{head}{tail}").into_bytes());

    // Short-form length: 0x30 0x03 and three content bytes, the last 0x00.
    assert_eq!(der_len(&[0x30, 0x03, 0x01, 0x02, 0x00, 0x00, 0x00]), 5);
}

fn openssl_path() -> Option<&'static str> {
    ["/opt/homebrew/bin/openssl", "/usr/local/bin/openssl", "openssl"]
        .into_iter()
        .find(|p| Command::new(p).arg("version").output().is_ok_and(|o| o.status.success()))
}

/// `Sample PDFs/vibepdf-verify-signed-then-edited.pdf` for the sweep: a signed
/// file with a note added and saved, so a human can confirm another reader
/// still calls the signature valid and reports the later change.
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn writes_verification_artifact() {
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../Sample PDFs/out/p6-security/vibepdf-verify-signed-then-edited.pdf");
    std::fs::write(&out, signed_hello(None)).expect("write signed");
    let handle = open(out.clone(), None);
    add_note(&handle).await;
    handle.save(None).await.expect("save");
    drop(handle);
    let _ = std::fs::remove_file(out.with_extension("pdf.bak"));
    println!("wrote {}", out.display());
}
