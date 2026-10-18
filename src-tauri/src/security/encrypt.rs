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

use lopdf::encryption::crypt_filters::{Aes256CryptFilter, CryptFilter};
use lopdf::{Document, EncryptionState, EncryptionVersion, Object, Permissions, StringFormat};

use crate::error::CommandError;
use crate::pdf::document::load_cos;

/// AES-256 file encryption key length, in bytes. Fixed by the V5 handler.
const KEY_BYTES: usize = 32;

/// The crypt-filter name written into `/Encrypt`. `StdCF` is the conventional
/// choice and what every reader expects to find.
const FILTER_NAME: &[u8] = b"StdCF";

// There are two `/Length` entries a V5 `/Encrypt` can carry, in different
// places and different units, and they must be treated oppositely.
//
// **The top-level one is not written.** PDF 2.0 makes it optional for V5 —
// `/AESV3` already states the key size — and adding it does active harm:
// `lopdf`'s own decrypt derives `n = Length / 8`, hits its `n > 16` guard (the
// legacy MD5 path caps at 128 bits) and fails with `InvalidKeyLength`. A
// `/Length 256` was briefly added there on a guess; it made our own output
// undecryptable by the very library that wrote it, which P6.C2 depends on.
//
// **The crypt filter's is written** — see `state_crypt_filter_key_length`.
// Leaving it out is what made Preview render every page of our files blank.

/// The crypt filter's key length, in **bytes**: the standard security handler
/// states a crypt filter's `/Length` in multiples of 8 ("16 means 128", ISO
/// 32000-1 Table 25), so AES-256 is 32, not 256. Every reader we test accepts
/// either; 32 is the one the standard names.
#[allow(clippy::cast_possible_wrap)] // 32 fits in an i64 with room to spare
const CRYPT_FILTER_KEY_LENGTH: i64 = KEY_BYTES as i64;

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

/// Write `/Length` into the crypt filter, where `CoreGraphics` requires it.
///
/// Without it, `PDFKit` — Preview, Safari, Quick Look — accepts the correct
/// password and then renders every page **blank**, logging `unsupported crypt
/// filter key length`. Nothing fails loudly: the document opens, it is simply
/// empty. `PDFium`, PDF.js and Ghostscript all derive the key size from `/AESV3`
/// and never noticed, which is how a file that looked correct in three readers
/// was unreadable in the one most Mac users open by default.
///
/// This is the crypt filter's entry, not the top-level `/Encrypt` `/Length`
/// that `lopdf` rejects (see the comment above `CRYPT_FILTER_KEY_LENGTH`), and
/// `security::decrypt` still unlocks the result.
fn state_crypt_filter_key_length(doc: &mut Document) -> Result<(), CommandError> {
    let id = doc
        .trailer
        .get(b"Encrypt")
        .and_then(Object::as_reference)
        .map_err(|e| CommandError::PdfError(format!("no /Encrypt after encrypting: {e}")))?;
    let filter = doc
        .get_object_mut(id)
        .and_then(Object::as_dict_mut)
        .and_then(|encrypt| encrypt.get_mut(b"CF"))
        .and_then(Object::as_dict_mut)
        .and_then(|filters| filters.get_mut(FILTER_NAME))
        .and_then(Object::as_dict_mut)
        .map_err(|e| CommandError::PdfError(format!("no crypt filter after encrypting: {e}")))?;
    filter.set("Length", Object::Integer(CRYPT_FILTER_KEY_LENGTH));
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
    // always has a credential with full rights. This is what a mainstream reader does when
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

    let mut doc = load_cos(bytes, |e| CommandError::PdfError(format!("could not read the PDF: {e}")))?;

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

    // Computed once and handed to `lopdf`, which writes it twice: as `/P`, and
    // encrypted inside the `/Perms` block. A reader that validates `/Perms`
    // compares the two and refuses the document when they differ — a password
    // error on the right password — so both come from this one value.
    //
    // This module used to rewrite `/Perms` itself, because `lopdf` 0.36 wrote
    // the block in plaintext. 0.37 fixed that upstream, and the rewrite had
    // become a second writer of a value that must match the first.
    // `tests/encrypt.rs::the_perms_entry_is_encrypted` checks `lopdf`'s block
    // against `/P`, and is what fails if a `lopdf` upgrade breaks it again.
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

    state_crypt_filter_key_length(&mut doc)?;

    let mut out = Vec::new();
    doc.save_to(&mut out)
        .map_err(|e| CommandError::PdfError(format!("could not write the encrypted PDF: {e}")))?;
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    fn restricted() -> DocumentPermissions {
        DocumentPermissions {
            print: false,
            copy: false,
            ..DocumentPermissions::default()
        }
    }

    // Not `#[derive(Default)]`: that would clear every bit and produce the most
    // restricted document possible for a caller who set nothing.
    #[test]
    fn the_default_allows_everything() {
        assert_eq!(
            DocumentPermissions::default().to_flags().bits(),
            Permissions::all().bits(),
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
