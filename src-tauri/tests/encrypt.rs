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
use vibepdf_lib::security::encrypt::{encrypt_document, DocumentPermissions, EncryptOptions};

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
        permissions: DocumentPermissions::default(),
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

// A deliberate narrowing of P6-SEC-007, not an oversight. The spec allows a
// document with only an owner password — opens for anyone, restricted — and we
// refuse to write one, because P6.C2 cannot unlock it: lopdf tries the empty
// user password while parsing and its R6 user authentication does not work, so
// the file cannot even be loaded.
//
// Producing files we cannot undo is a worse failure than a missing option, and
// the user would meet it later, on a document they can no longer change.
#[test]
fn refuses_owner_only_protection_because_it_could_not_be_undone() {
    let err = encrypt_document(&fixture_bytes("hello.pdf"), &opts(None, Some("let-me-change-it")))
        .unwrap_err();
    let CommandError::InvalidInput(msg) = &err else { panic!("got {err:?}") };
    assert!(
        msg.contains("remove that protection"),
        "the message must say why, not just refuse: {msg}"
    );
}

// Both passwords still work together, and the owner one is a real credential.
#[test]
fn supports_a_distinct_owner_password() {
    let out = encrypt_document(&fixture_bytes("hello.pdf"), &opts(Some("open-me"), Some("owner")))
        .expect("encrypt");
    let file = TempPdf::new(&out);

    assert!(open_pdf(&file.0, None).is_err(), "still gated on opening");
    let (doc, _) = open_pdf(&file.0, Some("open-me")).expect("user opens it");
    drop(doc);
    let (doc, _) = open_pdf(&file.0, Some("owner")).expect("owner opens it too");
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
    let err = encrypt_document(&fixture_bytes("hello.pdf"), &opts(Some(""), Some("owner"))).unwrap_err();
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

/// SPEC: P6-SEC-007 — a file to open in a mainstream reader / Preview / a third reader.
#[test]
#[ignore = "produces a verification artifact; run on demand"]
fn writes_verification_artifacts() {
    let dir = PathBuf::from("../Sample PDFs");
    std::fs::create_dir_all(&dir).expect("ensure Sample PDFs dir");
    let src = fixture_bytes("hello.pdf");

    for (name, o) in [
        ("vibepdf-verify-encrypted-user.pdf", opts(Some("open-me"), None)),
        ("vibepdf-verify-encrypted-both.pdf", opts(Some("open-me"), Some("owner-only"))),
        // SPEC: P6-SEC-009 — the acceptance demo from the roadmap: "open with
        // the user password: print is blocked". Printing and copying are the two
        // restrictions a reader is most likely to actually enforce, so they are
        // the ones worth putting in front of a mainstream reader.
        ("vibepdf-verify-no-print.pdf", {
            let mut o = opts(Some("open-me"), Some("owner-only"));
            o.permissions = DocumentPermissions {
                print: false,
                copy: false,
                ..DocumentPermissions::default()
            };
            o
        }),
    ] {
        let out = encrypt_document(&src, &o).expect("encrypt");
        let path = dir.join(name);
        std::fs::write(&path, &out).expect("write");
        eprintln!("wrote {}", path.display());
    }
}

// ---------------------------------------------------------------------------
// P6.C3 — permissions (SPEC: P6-SEC-009)
// ---------------------------------------------------------------------------
//
// "WHEN the user sets permissions, THE system SHALL allow restricting: print,
// copy, modify, fill forms, annotate, extract, assemble."
//
// What these can check is that the *document says* what the user chose. What
// they cannot check is that any reader obeys it — nothing in a PDF enforces
// permissions, PDFium largely ignores them, and a mainstream reader is the only honest
// acceptance test. That is a sweep item, not a unit test, and pretending
// otherwise here would be the more dangerous mistake.

/// The seven, one restricted at a time, against the bit each should clear.
fn one_restriction(f: impl Fn(&mut DocumentPermissions)) -> lopdf::Dictionary {
    let mut perms = DocumentPermissions::default();
    f(&mut perms);
    let mut o = opts(Some("open-me"), Some("owner-only"));
    o.permissions = perms;
    encrypt_dict(&encrypt_document(&fixture_bytes("hello.pdf"), &o).expect("encrypt"))
}

#[test]
fn p_records_the_permissions_that_were_chosen() {
    let perms = DocumentPermissions {
        print: false,
        copy: false,
        ..DocumentPermissions::default()
    };

    let mut o = opts(Some("open-me"), Some("owner-only"));
    o.permissions = perms;
    let dict = encrypt_dict(&encrypt_document(&fixture_bytes("hello.pdf"), &o).expect("encrypt"));

    // Compared against `p_value()` rather than a literal: the reserved bits are
    // the standard's business, and hard-coding them here would pin our own
    // arithmetic rather than the requirement.
    assert_eq!(int(&dict, b"P"), perms.to_flags().p_value() as i64);
}

#[test]
fn an_unrestricted_document_grants_everything() {
    let dict = encrypt_dict(
        &encrypt_document(
            &fixture_bytes("hello.pdf"),
            &opts(Some("open-me"), Some("owner-only")),
        )
        .expect("encrypt"),
    );
    assert_eq!(int(&dict, b"P"), lopdf::Permissions::all().p_value() as i64);
}

#[test]
fn every_restriction_in_the_spec_reaches_the_document() {
    use lopdf::Permissions as F;
    /// One spec-named restriction: how to switch it off, the bit it clears, and
    /// what to call it when it does not.
    type Case = (fn(&mut DocumentPermissions), F, &'static str);
    let cases: [Case; 7] = [
        (|p| p.print = false, F::PRINTABLE, "print"),
        (|p| p.copy = false, F::COPYABLE, "copy"),
        (|p| p.modify = false, F::MODIFIABLE, "modify"),
        (|p| p.fill_forms = false, F::FILLABLE, "fill forms"),
        (|p| p.annotate = false, F::ANNOTABLE, "annotate"),
        (
            |p| p.extract = false,
            F::COPYABLE_FOR_ACCESSIBILITY,
            "extract",
        ),
        (|p| p.assemble = false, F::ASSEMBLABLE, "assemble"),
    ];

    for (restrict, bit, name) in cases {
        let p = int(&one_restriction(restrict), b"P") as u64;
        assert_eq!(p & bit.bits(), 0, "restricting {name} left its bit set");
    }
}

// A restricted document must still *open*, and still be undoable by P6.C2.
// Permissions ride in the same `/Encrypt` dictionary as the passwords, and a
// wrong `/P` is one of the things that can make a reader reject the lot.
#[test]
fn a_restricted_document_still_opens_and_still_unlocks() {
    let mut o = opts(Some("open-me"), Some("owner-only"));
    o.permissions = DocumentPermissions {
        print: false,
        copy: false,
        ..DocumentPermissions::default()
    };
    let encrypted = encrypt_document(&fixture_bytes("hello.pdf"), &o).expect("encrypt");

    let temp = TempPdf::new(&encrypted);
    open_pdf(&temp.0, Some("open-me")).expect("PDFium opens a restricted document");

    vibepdf_lib::security::decrypt::remove_protection(&encrypted, "owner-only")
        .expect("P6.C2 can still undo it");
}
