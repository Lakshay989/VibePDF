//! Integration tests for password protection (P6.C1).
//!
//! SPEC: P6-SEC-007 — "THE system SHALL support both user password (open) and
//! owner password (permissions) with 256-bit AES encryption."
//!
//! The cryptography is `lopdf`'s, not ours, so these tests are not trying to
//! validate AES. They check the things a wiring mistake would break, each of
//! which fails *silently* in the sense that matters — the file still opens in a
//! reader, and only a careful look tells you it is not protected:
//!
//!   - is the `/Encrypt` dictionary actually V5/R6/256, or a weaker handler;
//!   - does the user password genuinely gate opening, per PDFium;
//!   - does an owner-only document open freely, as the spec intends;
//!   - is the key random, or the same on every run.
//!
//! The last is the one worth writing carefully. A fixed key produces a document
//! that looks encrypted to every reader and to every other test here.

use std::path::PathBuf;

use lopdf::{Document, Object};
use vibepdf_lib::error::CommandError;
use vibepdf_lib::pdf::document::open_pdf;
use vibepdf_lib::security::encrypt::{encrypt_document, EncryptOptions};

fn fixture_bytes(name: &str) -> Vec<u8> {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    std::fs::read(p).expect("read fixture")
}

/// A temp file that removes itself, so PDFium can be pointed at the output.
struct TempPdf(PathBuf);

impl TempPdf {
    fn new(bytes: &[u8]) -> Self {
        let p = std::env::temp_dir().join(format!("vibepdf-enc-{}.pdf", uuid::Uuid::new_v4()));
        std::fs::write(&p, bytes).expect("write temp pdf");
        Self(p)
    }
}

impl Drop for TempPdf {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn opts(user: Option<&str>, owner: Option<&str>) -> EncryptOptions {
    EncryptOptions {
        user_password: user.map(str::to_string),
        owner_password: owner.map(str::to_string),
    }
}

/// The `/Encrypt` dictionary of an encrypted document.
fn encrypt_dict(bytes: &[u8]) -> lopdf::Dictionary {
    let doc = Document::load_mem(bytes).expect("load");
    doc.get_encrypted().expect("an /Encrypt dictionary").clone()
}

fn int(dict: &lopdf::Dictionary, key: &[u8]) -> i64 {
    dict.get(key)
        .and_then(Object::as_i64)
        .unwrap_or_else(|_| panic!("missing /{}", String::from_utf8_lossy(key)))
}

// SPEC: P6-SEC-007 — "256-bit AES", which is the V5/R6 handler and nothing else.
#[test]
fn encrypts_with_aes_256() {
    let out = encrypt_document(&fixture_bytes("hello.pdf"), &opts(Some("open-me"), None))
        .expect("encrypt");
    let dict = encrypt_dict(&out);

    assert_eq!(int(&dict, b"V"), 5, "/V 5 is the AES-256 security handler");
    assert_eq!(int(&dict, b"R"), 6, "/R 6 is its revision");

    // V5 omits /Length — the key size is stated by the crypt filter's method,
    // and /AESV3 *is* the 256-bit one. Asserting /Length here would be
    // asserting a key the spec does not put in this dictionary.
    let cf = dict.get(b"CF").and_then(Object::as_dict).expect("/CF");
    let std = cf.get(b"StdCF").and_then(Object::as_dict).expect("/StdCF");
    assert_eq!(
        std.get(b"CFM").and_then(Object::as_name).expect("/CFM"),
        b"AESV3",
        "/AESV3 is AES-256; /AESV2 would be 128-bit and off-spec here"
    );
    assert_eq!(dict.get(b"StmF").and_then(Object::as_name).unwrap(), b"StdCF");
    assert_eq!(dict.get(b"StrF").and_then(Object::as_name).unwrap(), b"StdCF");

    // …and no `/Length`. Optional for V5, and actively harmful: lopdf's own
    // decrypt derives `n = Length / 8` and rejects `n > 16`, so writing it makes
    // our output undecryptable by the library that produced it — which is what
    // P6.C2 needs to do.
    assert!(dict.get(b"Length").is_err(), "/Length must not be written for V5");
}

// The regression test for the upstream defect this module works around.
//
// lopdf 0.36.0's `compute_permissions` encrypts a temporary copy and returns
// the plaintext, so `/Perms` goes out with `'T'` and `"adb"` sitting in the
// clear. PDFium validates that entry and refuses the entire document — a
// password error on a correct password. If this assertion ever fails, the
// workaround in `permissions_entry` has stopped being applied and the files we
// produce will open in some readers and not others.
#[test]
fn the_perms_entry_is_encrypted() {
    let out = encrypt_document(&fixture_bytes("hello.pdf"), &opts(Some("pw"), None)).expect("encrypt");
    let dict = encrypt_dict(&out);
    let perms = dict.get(b"Perms").and_then(Object::as_str).expect("/Perms");

    assert_eq!(perms.len(), 16, "Algorithm 10 produces exactly one AES block");
    assert_ne!(
        &perms[9..12],
        b"adb",
        "/Perms is the plaintext permission block — the lopdf bug is back"
    );
    // Byte 8 is 'T'/'F' before encryption; seeing either there is the same tell.
    assert!(
        perms[8] != b'T' && perms[8] != b'F',
        "/Perms looks unencrypted at byte 8"
    );
}

// SPEC: P6-SEC-007 — a user password gates opening. Checked through PDFium,
// because a reader's opinion is the only one that counts here.
#[test]
fn the_user_password_is_required_to_open() {
    let out = encrypt_document(&fixture_bytes("hello.pdf"), &opts(Some("open-me"), None))
        .expect("encrypt");
    let file = TempPdf::new(&out);

    assert!(
        open_pdf(&file.0, None).is_err(),
        "opens with no password — the protection is decorative"
    );
    assert!(open_pdf(&file.0, Some("wrong")).is_err(), "opens with the wrong password");
    let (doc, meta) = open_pdf(&file.0, Some("open-me")).expect("opens with the right password");
    assert_eq!(meta.page_count, 1);
    drop(doc);
}

// SPEC: P6-SEC-007 — the owner password is the *permissions* password; a
// document carrying only one still opens for anybody.
#[test]
fn an_owner_only_document_opens_without_a_password() {
    let out = encrypt_document(&fixture_bytes("hello.pdf"), &opts(None, Some("let-me-change-it")))
        .expect("encrypt");
    let file = TempPdf::new(&out);

    let (doc, meta) = open_pdf(&file.0, None).expect("opens freely");
    assert_eq!(meta.page_count, 1);
    drop(doc);
    // …and the owner password opens it too, which is what makes it a credential
    // rather than a label.
    let (doc, _) = open_pdf(&file.0, Some("let-me-change-it")).expect("owner opens it");
    drop(doc);
}

// SPEC: P6-SEC-007 — protection must not cost the document.
#[test]
fn the_content_survives_encryption() {
    let out = encrypt_document(&fixture_bytes("hello.pdf"), &opts(Some("pw"), None)).expect("encrypt");
    let file = TempPdf::new(&out);

    let (doc, _) = open_pdf(&file.0, Some("pw")).expect("open");
    let runs = vibepdf_lib::pdf::text_extract::extract_text_runs(&doc, 0).expect("extract");
    let text: String = runs.iter().map(|r| r.text.as_str()).collect();
    assert!(
        text.contains("Hello"),
        "the page text should survive encryption verbatim, got {text:?}"
    );
    drop(doc);
}

// The key must come from the OS, not from a constant or the passwords. A fixed
// key produces a file that looks encrypted to every reader and to every other
// test in this file — this is the only one that would notice.
#[test]
fn each_run_uses_a_fresh_key() {
    let src = fixture_bytes("hello.pdf");
    let a = encrypt_document(&src, &opts(Some("same"), Some("same"))).expect("first");
    let b = encrypt_document(&src, &opts(Some("same"), Some("same"))).expect("second");

    assert_ne!(a, b, "identical input and passwords produced identical bytes — the key is not random");

    // Narrower: /U is derived from the key, so it must differ even though the
    // password did not.
    let (da, db) = (encrypt_dict(&a), encrypt_dict(&b));
    let u = |d: &lopdf::Dictionary| d.get(b"U").and_then(Object::as_str).map(<[u8]>::to_vec).expect("/U");
    assert_ne!(u(&da), u(&db), "/U repeated across runs");
}

// Refusing is the only honest answer: the alternative announces itself as
// encrypted in the file structure and opens for anyone.
#[test]
fn refuses_when_no_password_was_given() {
    let err = encrypt_document(&fixture_bytes("hello.pdf"), &opts(None, None)).unwrap_err();
    assert!(matches!(err, CommandError::InvalidInput(_)), "got {err:?}");
    // Empty strings are the same thing arriving from a form.
    let err = encrypt_document(&fixture_bytes("hello.pdf"), &opts(Some(""), Some(""))).unwrap_err();
    assert!(matches!(err, CommandError::InvalidInput(_)), "got {err:?}");
}

// Re-encrypting would need the existing password to read the document first;
// that is P6.C2's job, and guessing here would produce nonsense.
#[test]
fn refuses_a_document_that_is_already_protected() {
    let once = encrypt_document(&fixture_bytes("hello.pdf"), &opts(Some("pw"), None)).expect("first");
    let err = encrypt_document(&once, &opts(Some("other"), None)).unwrap_err();
    assert!(matches!(err, CommandError::InvalidInput(_)), "got {err:?}");
}

// SPEC: P6-SEC-008 (P6.C2) depends on this: whatever we encrypt, we must be
// able to decrypt again. Cheap to assert here, and it is the property that a
// stray `/Length` silently destroyed.
#[test]
fn our_own_output_can_be_decrypted_again() {
    let out = encrypt_document(&fixture_bytes("hello.pdf"), &opts(Some("pw"), None)).expect("encrypt");
    let mut doc = Document::load_mem(&out).expect("load");
    assert!(doc.is_encrypted());
    doc.decrypt("pw").expect("lopdf must be able to decrypt what it wrote");
}

/// SPEC: P6-SEC-007 — a file to open in Acrobat / Preview / a third reader.
#[test]
#[ignore = "produces a verification artifact; run on demand"]
fn writes_verification_artifacts() {
    let dir = PathBuf::from("../Sample PDFs");
    std::fs::create_dir_all(&dir).expect("ensure Sample PDFs dir");
    let src = fixture_bytes("hello.pdf");

    for (name, o) in [
        ("vibepdf-verify-encrypted-user.pdf", opts(Some("open-me"), None)),
        ("vibepdf-verify-encrypted-owner.pdf", opts(None, Some("owner-only"))),
        ("vibepdf-verify-encrypted-both.pdf", opts(Some("open-me"), Some("owner-only"))),
    ] {
        let out = encrypt_document(&src, &o).expect("encrypt");
        let path = dir.join(name);
        std::fs::write(&path, &out).expect("write");
        eprintln!("wrote {}", path.display());
    }
}
