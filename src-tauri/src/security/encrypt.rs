//! SPEC: P6-SEC-007 (P6.C1) — password-protect a PDF with AES-256.
//!
//! **No cryptography is implemented here.** `lopdf` already ships the standard
//! security handler, including `EncryptionVersion::V5` — PDF 2.0, AES-256, the
//! R6 key derivation — and its `Permissions` bitflags. This file chooses the
//! parameters and hands over. That distinction is the whole point: a
//! hand-rolled AES-256 with R6 hashing would be the single most dangerous
//! thing in this codebase, wrong in ways no test here would notice.
//!
//! What *is* decided here, and is worth reviewing carefully:
//!
//! - the key comes from the OS CSPRNG (`getrandom`) and nowhere else;
//! - a user password is **required**, because P6.C2 cannot unlock a document
//!   that has only an owner password — see the note on `EncryptOptions`;
//! - an omitted owner password falls back to the user password, so the
//!   document always has a credential that can change its permissions.

use std::collections::BTreeMap;
use std::sync::Arc;

use aes::cipher::generic_array::GenericArray;
use aes::cipher::{BlockEncrypt, KeyInit};
use aes::Aes256;
use lopdf::encryption::crypt_filters::{Aes256CryptFilter, CryptFilter};
use lopdf::{Document, EncryptionState, EncryptionVersion, Object, Permissions, StringFormat};

use crate::error::CommandError;

/// AES-256 file encryption key length, in bytes. Fixed by the V5 handler.
const KEY_BYTES: usize = 32;

/// The crypt-filter name written into `/Encrypt`. `StdCF` is the conventional
/// choice and what every reader expects to find.
const FILTER_NAME: &[u8] = b"StdCF";

// No `/Length` is written. PDF 2.0 makes it optional for V5 — `/AESV3` already
// states the key size — and adding it does active harm: `lopdf`'s own decrypt
// derives `n = Length / 8`, hits its `n > 16` guard (the legacy MD5 path caps
// at 128 bits) and fails with `InvalidKeyLength`. A `/Length 256` was briefly
// added here on a guess while chasing an unrelated problem; it was never needed
// — `PDFium` opens these files without it — and it made our own output
// undecryptable by the very library that wrote it, which P6.C2 depends on.

/// What to protect a document with.
///
/// Both are optional and mean different things, which is why neither defaults
/// to the other in the UI:
///
/// - **user password** — required to *open* the document at all;
/// - **owner password** — required to change permissions; the document still
///   opens without it.
///
/// **A user password is currently required.** The spec permits owner-only
/// protection — a document that opens for anyone but is restricted — and this
/// deliberately does not, because P6.C2 cannot remove protection from such a
/// file: `lopdf` tries the empty user password while parsing and its R6 user
/// authentication does not work, so the document cannot even be loaded.
///
/// Writing files we cannot subsequently unlock is a worse failure than a
/// missing option, and it is one the user would only discover later, on a
/// document they can no longer change. Recorded in `steps/P6.md`; revisit when
/// the upstream R6 user-authentication path is fixed.
#[derive(Debug, Clone, Default)]
pub struct EncryptOptions {
    pub user_password: Option<String>,
    pub owner_password: Option<String>,
    pub permissions: DocumentPermissions,
}

/// SPEC: P6-SEC-009 (P6.C3) — what a reader may do with the protected document.
///
/// The seven the spec names, in its own words rather than the standard's, and
/// each `true` meaning *allowed*. The mapping to `lopdf`'s bitflags is in
/// [`DocumentPermissions::to_flags`]; that is the only place the PDF bit
/// numbering appears, so the rest of the codebase never has to reason about a
/// value where clearing a bit removes a right.
///
/// **These restrictions are advisory.** Nothing in a PDF enforces them
/// cryptographically — the content is decrypted either way, and a reader is
/// free to ignore `/P` entirely (`PDFium` largely does). They express intent to
/// readers that choose to honour it. Worse, if the owner password equals the
/// user password — which is what an omitted owner password gives you — then
/// anyone who can open the document can also lift the restrictions, so the
/// setting is close to meaningless without a *distinct* permissions password.
/// The UI says so; this comment is here so the next reader of this file does
/// not mistake it for security.
// `struct_excessive_bools` wants a state machine or two-variant enums. Not
// here: these are seven independent flags fixed by the PDF standard and named
// one-for-one by P6-SEC-009, not states of one thing. An enum per field would
// add fourteen type names to say what `bool` already says.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentPermissions {
    pub print: bool,
    pub copy: bool,
    pub modify: bool,
    pub fill_forms: bool,
    pub annotate: bool,
    pub extract: bool,
    pub assemble: bool,
}

/// Everything allowed — a protected document that restricts nothing.
///
/// Deliberately not `#[derive(Default)]`: that would give `false` for every
/// field, so a caller who left the permissions off would silently produce the
/// most restricted document possible instead of the least.
impl Default for DocumentPermissions {
    fn default() -> Self {
        Self {
            print: true,
            copy: true,
            modify: true,
            fill_forms: true,
            annotate: true,
            extract: true,
            assemble: true,
        }
    }
}

impl DocumentPermissions {
    /// Translate to the `/P` bitflags.
    ///
    /// `PRINTABLE_IN_HIGH_QUALITY` (bit 12) follows `print` rather than being a
    /// separate switch: on its own it only selects *degraded* printing when
    /// bit 3 is also set, which is a distinction worth neither a checkbox nor
    /// the explanation it would need.
    #[must_use]
    pub fn to_flags(self) -> Permissions {
        let mut p = Permissions::empty();
        p.set(
            Permissions::PRINTABLE | Permissions::PRINTABLE_IN_HIGH_QUALITY,
            self.print,
        );
        p.set(Permissions::COPYABLE, self.copy);
        p.set(Permissions::MODIFIABLE, self.modify);
        p.set(Permissions::FILLABLE, self.fill_forms);
        p.set(Permissions::ANNOTABLE, self.annotate);
        p.set(Permissions::COPYABLE_FOR_ACCESSIBILITY, self.extract);
        p.set(Permissions::ASSEMBLABLE, self.assemble);
        p
    }
}

/// A fresh 256-bit file encryption key from the operating system.
///
/// `getrandom` and not a seeded PRNG, and certainly not anything derived from
/// the passwords or the clock: this key *is* the confidentiality of the
/// document. A predictable key produces a file that looks encrypted in every
/// reader and is trivially opened by anyone who knows how it was generated —
/// the failure would be invisible to us and to the user both.
fn file_encryption_key() -> Result<[u8; KEY_BYTES], CommandError> {
    let mut key = [0u8; KEY_BYTES];
    getrandom::fill(&mut key)
        .map_err(|e| CommandError::Internal(format!("could not obtain random bytes: {e}")))?;
    Ok(key)
}

/// Give the document a trailer `/ID` if it has none.
///
/// Two 16-byte strings: the "original" and "current" identifiers. For a file
/// being protected now they are the same value — the pair only diverges when a
/// document is later updated incrementally, which this path does not do.
///
/// `/ID` is never encrypted, so random bytes are both sufficient and correct;
/// the older MD5-of-metadata recipe in the spec is only a suggestion for making
/// the value unique, and a CSPRNG does that better.
fn ensure_file_id(doc: &mut Document) -> Result<(), CommandError> {
    if doc.trailer.has(b"ID") {
        return Ok(());
    }
    let mut id = [0u8; 16];
    getrandom::fill(&mut id)
        .map_err(|e| CommandError::Internal(format!("could not obtain random bytes: {e}")))?;
    let entry = Object::String(id.to_vec(), StringFormat::Hexadecimal);
    doc.trailer
        .set("ID", Object::Array(vec![entry.clone(), entry]));
    Ok(())
}

/// ISO 32000-2 Algorithm 10 — the `/Perms` block, encrypted.
///
/// **This exists because `lopdf` 0.36.0 gets it wrong.** Its
/// `compute_permissions` builds the 16-byte block correctly and then does
/// `encrypt_block_mut(&mut bytes.into())` — `into()` materialises a *temporary*
/// `GenericArray`, the encryption lands in that temporary, and the function
/// returns the untouched plaintext. The `/Perms` it writes reads literally
/// `…54 61 64 62 …` — `'T'`, `"adb"` — in the clear.
///
/// The consequence is not cosmetic. `PDFium` validates `/Perms` and **refuses the
/// whole document**, reporting a password error even when the password is
/// right; `pypdf` logs "ignore '/Perms' verify failed" and carries on. A file
/// that some readers open and others reject, with nothing on screen to say
/// which, is exactly the failure `security/` exists to prevent.
///
/// This is not cryptographic design: no key derivation, no mode selection, no
/// IV. One AES-256-ECB block over a layout the standard fixes exactly, with a
/// key that is already in hand. Note the deliberate shape of the call below —
/// a *named* block encrypted in place, then read back — which is the mistake
/// upstream made.
///
/// `p_value` is read back out of the `/P` that was actually written, rather
/// than passed in alongside it — see `fix_permissions_entry`.
fn permissions_entry(key: &[u8; KEY_BYTES], p_value: u64) -> Result<Vec<u8>, CommandError> {
    let mut block = [0u8; 16];
    // 0–7: the permission bits, low byte first.
    block[..8].copy_from_slice(&p_value.to_le_bytes());
    // 8: 'T' or 'F' for EncryptMetadata — we always encrypt it.
    block[8] = b'T';
    // 9–11: the literal marker a reader checks for after decrypting.
    block[9..12].copy_from_slice(b"adb");
    // 12–15: random filler, ignored on read.
    getrandom::fill(&mut block[12..16])
        .map_err(|e| CommandError::Internal(format!("could not obtain random bytes: {e}")))?;

    let cipher = Aes256::new(GenericArray::from_slice(key));
    let mut out = GenericArray::clone_from_slice(&block);
    cipher.encrypt_block(&mut out);
    Ok(out.to_vec())
}

/// Replace the `/Perms` entry lopdf just produced with a correctly encrypted one.
///
/// **The permission bits come from the document's own `/P`, not from a
/// parameter.** They are written twice — once as `/P`, once encrypted inside
/// `/Perms` — and a reader that validates `/Perms` compares the two. Taking
/// them as an argument alongside the set handed to `lopdf` would leave two
/// values that are *supposed* to be equal free to drift apart, and the symptom
/// would be a document some readers reject with nothing to say why. Deriving
/// one from the other makes that unrepresentable rather than merely tested.
fn fix_permissions_entry(doc: &mut Document, key: &[u8; KEY_BYTES]) -> Result<(), CommandError> {
    let id = doc
        .trailer
        .get(b"Encrypt")
        .and_then(Object::as_reference)
        .map_err(|e| CommandError::PdfError(format!("no /Encrypt after encrypting: {e}")))?;
    let dict = doc
        .get_object_mut(id)
        .and_then(Object::as_dict_mut)
        .map_err(|e| CommandError::PdfError(format!("/Encrypt is not a dictionary: {e}")))?;

    // `lopdf` writes `/P` as `p_value() as i64`. Going through the raw bytes
    // reinterprets those 64 bits rather than converting the number, which is
    // the point: `/P` is a bit field the standard happens to store in a signed
    // integer, and Algorithm 10 wants the bits verbatim in bytes 0–7.
    // (`i64::cast_unsigned` says this more directly but postdates our MSRV.)
    let signed = dict
        .get(b"P")
        .and_then(Object::as_i64)
        .map_err(|e| CommandError::PdfError(format!("no /P after encrypting: {e}")))?;
    let p_value = u64::from_ne_bytes(signed.to_ne_bytes());

    let perms = permissions_entry(key, p_value)?;
    let dict = doc
        .get_object_mut(id)
        .and_then(Object::as_dict_mut)
        .map_err(|e| CommandError::PdfError(format!("/Encrypt is not a dictionary: {e}")))?;
    dict.set("Perms", Object::String(perms, StringFormat::Literal));
    Ok(())
}

/// SPEC: P6-SEC-007 — return `bytes` re-encoded with AES-256 encryption.
///
/// The input must be an unencrypted document; re-encrypting an already
/// encrypted file is refused rather than guessed at (removing the existing
/// protection first is P6.C2, and needs its owner password).
///
/// SPEC: P6-SEC-009 — `opts.permissions` decides what the document allows.
pub fn encrypt_document(bytes: &[u8], opts: &EncryptOptions) -> Result<Vec<u8>, CommandError> {
    let user = opts.user_password.as_deref().unwrap_or("");
    // An omitted owner password becomes the user password, so the document
    // always has a credential with full rights. This is what Acrobat does when
    // only an open password is set.
    let owner = opts.owner_password.as_deref().unwrap_or(user);

    if user.is_empty() {
        return Err(CommandError::InvalidInput(
            "Set a password to open the document. Protecting a file with only a \
             permissions password isn't supported yet — VibePDF would not be able to \
             remove that protection afterwards."
                .into(),
        ));
    }

    let mut doc = Document::load_mem(bytes)
        .map_err(|e| CommandError::PdfError(format!("could not read the PDF: {e}")))?;

    if doc.is_encrypted() {
        return Err(CommandError::InvalidInput(
            "This PDF is already password protected. Remove the existing protection first.".into(),
        ));
    }

    // ISO 32000-1 §7.5.5: a document carrying `/Encrypt` **shall** have an
    // `/ID` in its trailer. `hello.pdf` and plenty of real PDFs have none, and
    // `lopdf` does not add one — so the first version of this produced a file
    // that `PDFium` rejected with "password required" *when given the correct
    // password*. The symptom points at the password and the cause is a missing
    // trailer entry two levels away.
    ensure_file_id(&mut doc)?;

    let key = file_encryption_key()?;
    let filter: Arc<dyn CryptFilter> = Arc::new(Aes256CryptFilter);

    // Computed once and used twice, on purpose. The permission bits are written
    // in two places — `/P`, and encrypted inside the `/Perms` block below — and
    // a reader that validates `/Perms` compares the two. Two calls to
    // `Permissions::all()` used to sit here instead; the moment the value stops
    // being a constant, that shape is a disagreement waiting to happen, and the
    // symptom would be a document some readers reject with no clue why.
    let permissions = opts.permissions.to_flags();

    let version = EncryptionVersion::V5 {
        // Encrypting the metadata too. Leaving it in the clear is permitted by
        // the spec and leaks title, author and producer out of a document whose
        // whole point is that it is private.
        encrypt_metadata: true,
        crypt_filters: BTreeMap::from([(FILTER_NAME.to_vec(), filter)]),
        file_encryption_key: &key,
        stream_filter: FILTER_NAME.to_vec(),
        string_filter: FILTER_NAME.to_vec(),
        owner_password: owner,
        user_password: user,
        permissions,
    };

    let state = EncryptionState::try_from(version)
        .map_err(|e| CommandError::PdfError(format!("could not set up encryption: {e}")))?;
    doc.encrypt(&state)
        .map_err(|e| CommandError::PdfError(format!("could not encrypt the document: {e}")))?;

    fix_permissions_entry(&mut doc, &key)?;

    let mut out = Vec::new();
    doc.save_to(&mut out)
        .map_err(|e| CommandError::PdfError(format!("could not write the encrypted PDF: {e}")))?;
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use aes::cipher::BlockDecrypt;

    /// Undo `permissions_entry`, so a test can read the bits a reader would.
    fn decode_perms(key: &[u8; KEY_BYTES], entry: &[u8]) -> [u8; 16] {
        let cipher = Aes256::new(GenericArray::from_slice(key));
        let mut block = GenericArray::clone_from_slice(entry);
        cipher.decrypt_block(&mut block);
        block.into()
    }

    fn restricted() -> DocumentPermissions {
        DocumentPermissions {
            print: false,
            copy: false,
            ..DocumentPermissions::default()
        }
    }

    // SPEC: P6-SEC-009 — the bits inside `/Perms` are the bits handed to it.
    //
    // This is the encoding only. That `/Perms` agrees with `/P` is not tested
    // here and does not need to be: `fix_permissions_entry` reads the value out
    // of `/P` itself, so there is no second value to disagree with. A test
    // passing the same flags to both sides would have proved nothing.
    #[test]
    fn perms_carries_the_bits_it_was_given() {
        let key = [7u8; KEY_BYTES];
        let p = restricted().to_flags().p_value();
        let block = decode_perms(&key, &permissions_entry(&key, p).expect("build /Perms"));

        assert_eq!(
            u64::from_le_bytes(block[..8].try_into().expect("8 bytes")),
            p
        );
        assert_eq!(block[8], b'T', "metadata is encrypted");
        assert_eq!(&block[9..12], b"adb");
    }

    // The upstream bug this whole function exists to work around: `lopdf`
    // returns the block unencrypted, so the marker survives in the clear.
    #[test]
    fn perms_is_actually_encrypted() {
        let key = [7u8; KEY_BYTES];
        let entry = permissions_entry(&key, restricted().to_flags().p_value()).expect("build");
        assert_ne!(&entry[9..12], b"adb");
    }

    // Not `#[derive(Default)]`: that would clear every bit and produce the most
    // restricted document possible for a caller who set nothing.
    #[test]
    fn the_default_allows_everything() {
        assert_eq!(
            DocumentPermissions::default().to_flags().p_value(),
            Permissions::all().p_value(),
        );
    }

    #[test]
    fn each_restriction_clears_its_own_bit_and_no_other() {
        let all = DocumentPermissions::default();
        let cases: [(DocumentPermissions, Permissions); 7] = [
            (
                DocumentPermissions {
                    print: false,
                    ..all
                },
                Permissions::PRINTABLE,
            ),
            (
                DocumentPermissions { copy: false, ..all },
                Permissions::COPYABLE,
            ),
            (
                DocumentPermissions {
                    modify: false,
                    ..all
                },
                Permissions::MODIFIABLE,
            ),
            (
                DocumentPermissions {
                    fill_forms: false,
                    ..all
                },
                Permissions::FILLABLE,
            ),
            (
                DocumentPermissions {
                    annotate: false,
                    ..all
                },
                Permissions::ANNOTABLE,
            ),
            (
                DocumentPermissions {
                    extract: false,
                    ..all
                },
                Permissions::COPYABLE_FOR_ACCESSIBILITY,
            ),
            (
                DocumentPermissions {
                    assemble: false,
                    ..all
                },
                Permissions::ASSEMBLABLE,
            ),
        ];

        for (perms, bit) in cases {
            let flags = perms.to_flags();
            assert!(
                !flags.contains(bit),
                "{perms:?} should have cleared {bit:?}"
            );
            // Everything else this document does not restrict is still granted.
            // High-quality printing is the one deliberate exception: it follows
            // `print`, so it is excluded from the comparison.
            let others = Permissions::all() - bit - Permissions::PRINTABLE_IN_HIGH_QUALITY;
            assert_eq!(
                flags & others,
                others,
                "{perms:?} cleared more than {bit:?}"
            );
        }
    }

    // Bit 12 selects *faithful* printing; on its own it only means "degraded
    // printing allowed", which is not a distinction worth a checkbox.
    #[test]
    fn high_quality_printing_follows_printing() {
        assert!(!restricted()
            .to_flags()
            .contains(Permissions::PRINTABLE_IN_HIGH_QUALITY));
    }
}
