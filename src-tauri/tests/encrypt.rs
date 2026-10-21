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
//!   - does an owner-only document open freely, as the spec intends, and still
//!     come out encrypted;
//!   - is the key random, or the same on every run.
//!
//! The last is the one worth writing carefully. A fixed key produces a document
//! that looks encrypted to every reader and to every other test here.

use std::path::PathBuf;

use aes::cipher::generic_array::GenericArray;
use aes::cipher::{BlockDecrypt, KeyInit};
use aes::Aes256;
use lopdf::{Document, EncryptionState, Object};
use vibepdf_lib::error::CommandError;
use vibepdf_lib::pdf::document::{declares_encryption, open_pdf};
use vibepdf_lib::security::decrypt::remove_protection;
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

    // The crypt filter names the method: /AESV3 is AES-256.
    let cf = dict.get(b"CF").and_then(Object::as_dict).expect("/CF");
    let std = cf.get(b"StdCF").and_then(Object::as_dict).expect("/StdCF");
    assert_eq!(
        std.get(b"CFM").and_then(Object::as_name).expect("/CFM"),
        b"AESV3",
        "/AESV3 is AES-256; /AESV2 would be 128-bit and off-spec here"
    );
    // …and states its key length, in bytes. Without this PDFKit (Preview,
    // Safari, Quick Look) accepts the correct password and then renders every
    // page blank — "unsupported crypt filter key length" — while PDFium, PDF.js
    // and Ghostscript never notice. Found by the P6 sweep, not by any test.
    assert_eq!(
        std.get(b"Length").and_then(Object::as_i64).expect("/StdCF /Length"),
        32,
        "the crypt filter must state a 32-byte key or Preview renders nothing"
    );
    assert_eq!(dict.get(b"StmF").and_then(Object::as_name).unwrap(), b"StdCF");
    assert_eq!(dict.get(b"StrF").and_then(Object::as_name).unwrap(), b"StdCF");

    // …and no top-level `/Length` — a different entry from the crypt filter's
    // above. Optional for V5, and actively harmful: lopdf's own
    // decrypt derives `n = Length / 8` and rejects `n > 16`, so writing it makes
    // our output undecryptable by the library that produced it — which is what
    // P6.C2 needs to do.
    assert!(dict.get(b"Length").is_err(), "/Length must not be written for V5");
}

// The guard on lopdf's own `/Perms` (ISO 32000-2 Algorithm 10).
//
// lopdf 0.36.0's `compute_permissions` encrypted a temporary copy and returned
// the plaintext, so `/Perms` went out with `'T'` and `"adb"` sitting in the
// clear. PDFium validates that entry and refuses the entire document — a
// password error on a correct password. `security/encrypt.rs` rewrote the
// entry to work around it until lopdf 0.37 fixed it upstream; now nothing of
// ours touches `/Perms`, and this test is what fails if a lopdf upgrade brings
// the bug back.
//
// Checked by decrypting the entry, not by looking at the ciphertext. A correct
// entry is one AES block under a random key, so any byte of it is 'T' or 'F'
// about one time in 128 — an earlier version asserted byte 8 was neither, and
// failed correct files at that rate. A block that decrypts under the file key
// to the Algorithm 10 layout is both encrypted and right; the plaintext block
// cannot decrypt to it.
//
// The key comes back through lopdf's R6 password path, not from the code that
// wrote `/Perms`: we generate the key and hand it over, so `/Perms` written
// under anything else decrypts to noise.
//
// The byte-by-byte asserts are not redundant with the PDFium open at the end.
// PDFium checks the `"adb"` marker and bytes 0–3 against `/P`, but accepts a
// block whose bytes 4–7 are not all ones or whose byte 8 says 'F' (measured
// 2026-10-18 by tampering with `/Perms`), so those bytes are checked here or
// nowhere.
#[test]
fn the_perms_entry_is_encrypted() {
    // Restricted, so the `/P` it must agree with is not the all-allowed default
    // that a hardcoded value would also produce.
    let mut o = opts(Some("pw"), None);
    o.permissions = DocumentPermissions {
        print: false,
        copy: false,
        ..DocumentPermissions::default()
    };
    let out = encrypt_document(&fixture_bytes("hello.pdf"), &o).expect("encrypt");

    let state = EncryptionState::decode(&Document::load_mem(&out).expect("load"), "pw")
        .expect("lopdf recovers the file key");
    let dict = encrypt_dict(&out);
    let perms = dict.get(b"Perms").and_then(Object::as_str).expect("/Perms");
    assert_eq!(perms.len(), 16, "Algorithm 10 produces exactly one AES block");

    // Algorithm 10 is ECB with one block: no IV, no padding.
    let mut block = GenericArray::clone_from_slice(perms);
    Aes256::new(GenericArray::from_slice(state.file_encryption_key())).decrypt_block(&mut block);

    assert_eq!(
        &block[9..12],
        b"adb",
        "/Perms does not decrypt under the file key{}",
        if &perms[9..12] == b"adb" {
            " — it is the plaintext block: lopdf's 0.36 /Perms bug is back"
        } else {
            ""
        }
    );
    // Bytes 0–3 are `/P`, low byte first; 4–7 are the all-ones extension to 64 bits.
    let mut expected = [0xFF; 8];
    expected[..4].copy_from_slice(&int(&dict, b"P").to_le_bytes()[..4]);
    assert_eq!(&block[..8], &expected, "/Perms disagrees with /P");
    assert_eq!(block[8], b'T', "metadata is encrypted, so byte 8 is 'T'");

    // The outside check: PDFium runs Algorithm 13 on open, and it is the reader
    // that refused the files the lopdf bug produced.
    let file = TempPdf::new(&out);
    let (doc, _) = open_pdf(&file.0, Some("pw")).expect("PDFium accepts /Perms");
    drop(doc);
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

/// `hello.pdf` protected with only a permissions password, printing and
/// copying restricted.
fn permissions_only(owner: &str) -> Vec<u8> {
    let mut o = opts(None, Some(owner));
    o.permissions = DocumentPermissions {
        print: false,
        copy: false,
        ..DocumentPermissions::default()
    };
    encrypt_document(&fixture_bytes("hello.pdf"), &o).expect("encrypt")
}

// SPEC: P6-SEC-007 — "both user password (open) and owner password
// (permissions)". With only the owner password, the copy opens for anyone and
// carries the restrictions. Opening with no password proves nothing about
// whether it is protected, so the dictionary is checked as well.
// (Refused until 2026-10-21, because Unlock could not undo it.)
#[test]
fn protects_with_only_a_permissions_password() {
    let out = permissions_only("let-me-change-it");
    assert!(declares_encryption(&out), "the copy is not encrypted");
    // lopdf opens a file with no open password using the empty one and drops
    // its dictionary on the way, so the values are read from the state it
    // decoded them into.
    let doc = Document::load_mem(&out).expect("load");
    let state = doc.encryption_state.as_ref().expect("lopdf decrypted it with the empty password");
    assert_eq!((state.version(), state.revision()), (5, 6), "not the AES-256 handler");
    // SPEC: P6-SEC-009 — print clears bits 3 and 12, copy clears bit 5.
    #[allow(clippy::cast_possible_wrap)] // /P is a signed 32-bit word, kept in a u64
    let p = state.permissions().bits() as i64;
    assert_eq!(p, spec_p(&[3, 5, 12]));

    let file = TempPdf::new(&out);
    let (doc, _) = open_pdf(&file.0, None).expect("opens with no password");
    let runs = vibepdf_lib::pdf::text_extract::extract_text_runs(&doc, 0).expect("extract");
    let text: String = runs.iter().map(|r| r.text.as_str()).collect();
    assert!(text.contains("Hello"), "the page text should survive, got {text:?}");
    drop(doc);
}

// SPEC: P6-SEC-008 — what Protect writes, Unlock must be able to undo, with the
// owner password and nothing else. Its absence is why this was refused.
#[test]
fn a_permissions_only_copy_unlocks_with_its_owner_password() {
    let out = permissions_only("let-me-change-it");
    for wrong in ["", "wrong", "let-me-change-i"] {
        match remove_protection(&out, wrong) {
            Err(CommandError::InvalidInput(message)) => {
                assert_eq!(message, "That password did not unlock the document.", "{wrong:?}");
            }
            other => panic!("{wrong:?} should be refused, got {other:?}"),
        }
    }
    let unlocked = remove_protection(&out, "let-me-change-it").expect("the owner password unlocks it");
    assert!(!declares_encryption(&unlocked), "the unlocked copy is still encrypted");
}

// An empty owner password is no owner password: it falls back to the open
// password, as an omitted one does. Taken literally, anyone could remove the
// protection with the empty password, and a reader that tries the empty
// password as the owner's would open a file that is meant to need one.
#[test]
fn an_empty_owner_password_falls_back_to_the_open_password() {
    let out = encrypt_document(&fixture_bytes("hello.pdf"), &opts(Some("open-me"), Some("")))
        .expect("encrypt");

    // lopdf tries the empty password on every load, as owner or user, so had
    // it worked this would come back decrypted, and the owner check below
    // would fail only because there was nothing left to check.
    let locked = Document::load_mem(&out).expect("load");
    assert!(locked.is_encrypted(), "the empty password opened it");
    assert!(
        locked.authenticate_owner_password("").is_err(),
        "the empty password is the owner password"
    );
    assert!(remove_protection(&out, "").is_err(), "the empty password removed the protection");
    let file = TempPdf::new(&out);
    assert!(open_pdf(&file.0, None).is_err(), "opens with no password");
    remove_protection(&out, "open-me").expect("the open password is the owner password too");
}

// A copy with only a permissions password and nothing restricted opens for
// anyone and limits nothing, while readers and editors treat it as protected.
#[test]
fn refuses_permissions_only_protection_that_restricts_nothing() {
    let err = encrypt_document(&fixture_bytes("hello.pdf"), &opts(None, Some("owner"))).unwrap_err();
    let CommandError::InvalidInput(msg) = &err else { panic!("got {err:?}") };
    assert!(msg.contains("restrict at least one permission"), "{msg}");
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
//
// Decrypted through the loader and checked for content. On lopdf 0.42 the old
// form — `load_mem`, then `decrypt` — still returns Ok, but on a parse that
// held only the encryption dictionary, so it "decrypted" a document with no
// pages and this test passed while checking nothing.
#[test]
fn our_own_output_can_be_decrypted_again() {
    let out = encrypt_document(&fixture_bytes("hello.pdf"), &opts(Some("pw"), None)).expect("encrypt");
    assert!(Document::load_mem(&out).expect("load").is_encrypted());

    let doc = Document::load_mem_with_options(&out, lopdf::LoadOptions::with_password("pw"))
        .expect("lopdf must be able to decrypt what it wrote");
    assert!(!doc.is_encrypted());
    assert_eq!(doc.get_pages().len(), 1, "decrypted, but the page is gone");
}

/// SPEC: P6-SEC-007 — a file to open in a mainstream reader / Preview / a third reader.
#[test]
#[ignore = "produces a verification artifact; run on demand"]
fn writes_verification_artifacts() {
    let dir = PathBuf::from("../Sample PDFs/out/p6-security");
    std::fs::create_dir_all(&dir).expect("ensure Sample PDFs dir");
    let src = fixture_bytes("hello.pdf");

    for (name, o) in [
        ("vibepdf-verify-encrypted-user.pdf", opts(Some("open-me"), None)),
        ("vibepdf-verify-encrypted-both.pdf", opts(Some("open-me"), Some("owner-only"))),
        // Opens with no password; printing and copying restricted, and only
        // `owner-only` lifts them.
        ("vibepdf-verify-permissions-only.pdf", {
            let mut o = opts(None, Some("owner-only"));
            o.permissions = DocumentPermissions {
                print: false,
                copy: false,
                ..DocumentPermissions::default()
            };
            o
        }),
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

    // Print clears bit 3 and its high-quality companion, bit 12; copy clears 5.
    assert_eq!(int(&dict, b"P"), spec_p(&[3, 5, 12]));
}

/// `/P` as ISO 32000-1 Table 22 defines it, given the bits a restriction
/// clears (numbered from 1, as the table numbers them).
///
/// Written from the table rather than taken from lopdf: these tests compared
/// against lopdf's own `p_value()` until 0.42 removed it, which checked that
/// lopdf agreed with itself. Bits 1–2 must be 0, every other bit is 1 unless
/// a restriction clears it, and the 32-bit word is stored as a signed integer.
fn spec_p(cleared: &[u32]) -> i64 {
    let mut word: u32 = !0b11;
    for bit in cleared {
        word &= !(1 << (bit - 1));
    }
    i64::from(i32::from_ne_bytes(word.to_ne_bytes()))
}

// The helper, worked by hand: everything allowed is 0xFFFFFFFC, i.e. -4 — the
// value most writers emit. Clearing bit 3 (4) and bit 12 (2048) as well gives
// -4 - 4 - 2048 = -2056.
#[test]
fn spec_p_matches_the_table_worked_by_hand() {
    assert_eq!(spec_p(&[]), -4);
    assert_eq!(spec_p(&[3, 12]), -2056);
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
    assert_eq!(int(&dict, b"P"), spec_p(&[]));
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
