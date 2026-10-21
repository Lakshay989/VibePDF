//! SPEC: P6-SEC-008 (P6.C2) — remove password protection from a PDF.
//!
//! The mirror of `encrypt.rs`, and like it, no cryptography of our own:
//! `lopdf` authenticates the password, decrypts every string and stream, and
//! drops both the `/Encrypt` trailer entry and the encryption dictionary. What
//! is left is an ordinary document.
//!
//! **"SHALL require the owner password" is checked here, for every handler.**
//! lopdf's loader accepts either password — it tries the empty one, then the
//! one it was given, as owner or user — so it cannot be what enforces the spec.
//! Up to lopdf 0.36 the AES-256 path happened to refuse the user password while
//! RC4 and AES-128 accepted it; since lopdf 0.42 (2026-10-17) the supplied
//! password is authenticated *as the owner password* before anything is
//! decrypted.
//!
//! That check reads the encryption dictionary and the trailer's `/ID`, and
//! lopdf discards the dictionary whenever it decrypts on load. A file that
//! needs an open password keeps it: parsed without a password, it comes back as
//! its trailer and that dictionary and nothing else. A **permissions-only**
//! file (no open password) does not — lopdf opens it with the empty password
//! and throws the dictionary away. What lopdf keeps is its decryption state,
//! which holds every value the check reads, and `EncryptionState::encode`
//! writes them back as a dictionary. The check runs on that, in a probe
//! document carrying nothing but it and the `/ID`. `tests/decrypt.rs` pins that
//! the rebuilt dictionary matches the original value for value wherever lopdf
//! keeps both. Until 2026-10-21 these files were refused, whatever the password.
//!
//! **The copy is checked for encryption before it is returned.** The command
//! re-opens it with no password, but a permissions-only file opens that way
//! while it is still protected, so the re-open alone would not notice.
//!
//! This file parses with lopdf directly rather than through
//! `pdf::document::load_cos`, because the encrypted, undecrypted document is
//! exactly what `load_cos` refuses and exactly what the check needs.
//! `tests/lopdf_load_policy.rs` allows it by name.

use lopdf::{Document, LoadOptions, Object};

use crate::error::CommandError;
use crate::pdf::document::declares_encryption;

/// A document that declares encryption, but whose protection lopdf can't read:
/// it kept neither the dictionary nor a state to rebuild it from.
const UNREADABLE: &str =
    "VibePDF can't read this document's password protection, so it can't remove it.";

/// Whether the document needs a password to open, which decides what a
/// password that isn't the owner's can be.
#[derive(Clone, Copy)]
enum OpenPassword {
    Required,
    /// Permissions-only: anyone can open it, so no password is "the one to
    /// open it".
    None,
}

/// SPEC: P6-SEC-008 — return `bytes` with its encryption removed.
///
/// `password` must be the **owner** password, whatever the encryption: the
/// user password proves only the right to read. The input must actually be
/// encrypted — a plain document is refused rather than silently rewritten,
/// because "removed protection from a file that had none" and "removed
/// protection" are indistinguishable afterwards.
pub fn remove_protection(bytes: &[u8], password: &str) -> Result<Vec<u8>, CommandError> {
    if !declares_encryption(bytes) {
        return Err(CommandError::InvalidInput(
            "This PDF is not password protected.".into(),
        ));
    }

    // Without a password, a file that needs one parses as its trailer and its
    // encryption dictionary — all the password check needs, and nothing that
    // could be saved by mistake. A permissions-only file parses decrypted, with
    // its dictionary gone, and is checked against the one rebuilt from what
    // lopdf kept.
    let parsed = Document::load_mem(bytes).map_err(|e| explain_load_failure(&e))?;
    if parsed.is_encrypted() {
        reject_unsupported_variant(&parsed)?;
        require_owner_password(&parsed, password, OpenPassword::Required)?;
    } else {
        let probe = rebuilt_encryption(&parsed)?;
        reject_unsupported_variant(&probe)?;
        require_owner_password(&probe, password, OpenPassword::None)?;
    }

    let mut doc = Document::load_mem_with_options(bytes, LoadOptions::with_password(password))
        .map_err(|e| {
            // The password was just authenticated, so this is not a wrong
            // password; and it must never appear in the message either way.
            tracing::debug!(error = ?e, "decrypting load failed");
            CommandError::PdfError("could not decrypt the PDF.".into())
        })?;

    let mut out = Vec::new();
    doc.save_to(&mut out)
        .map_err(|e| CommandError::PdfError(format!("could not write the unlocked PDF: {e}")))?;
    refuse_still_encrypted(&out)?;
    Ok(out)
}

/// A permissions-only document's encryption, rebuilt from the state lopdf kept
/// when it decrypted the file on load: the dictionary
/// `EncryptionState::encode` writes back, plus the trailer's `/ID`, which the
/// RC4 and AES-128 checks hash. The probe holds nothing else.
fn rebuilt_encryption(opened: &Document) -> Result<Document, CommandError> {
    let Some(state) = opened.encryption_state.as_ref() else {
        // It declares `/Encrypt`, yet lopdf neither kept a dictionary nor
        // decrypted anything with one.
        return Err(CommandError::InvalidInput(UNREADABLE.into()));
    };
    let dictionary = state.encode().map_err(|e| {
        tracing::debug!(error = ?e, "re-encoding the encryption state failed");
        CommandError::InvalidInput(UNREADABLE.into())
    })?;

    let mut probe = Document::new();
    if let Ok(id) = opened.trailer.get(b"ID") {
        probe.trailer.set("ID", id.clone());
    }
    let reference = probe.add_object(dictionary);
    probe.trailer.set("Encrypt", Object::Reference(reference));
    Ok(probe)
}

/// The supplied password, authenticated as the **owner** password.
///
/// On a document that needs an open password, the user password is named as
/// the wrong kind, since "did not unlock" would read as a typo to someone
/// holding the right password for a different purpose. A permissions-only
/// document has no such password, and the empty one that opens it must not be
/// described as one. The password never appears in a message.
fn require_owner_password(locked: &Document, password: &str, open: OpenPassword) -> Result<(), CommandError> {
    if locked.authenticate_owner_password(password).is_ok() {
        return Ok(());
    }
    if matches!(open, OpenPassword::Required) && locked.authenticate_user_password(password).is_ok() {
        return Err(CommandError::InvalidInput(
            "That is the password to open the document. Removing its protection needs the \
             owner (permissions) password."
                .into(),
        ));
    }
    Err(CommandError::InvalidInput(
        "That password did not unlock the document.".into(),
    ))
}

/// Refuse a copy that still declares encryption.
///
/// The command re-opens the copy with no password, which proves nothing for a
/// file that never needed one to open. This is the check that would notice.
fn refuse_still_encrypted(out: &[u8]) -> Result<(), CommandError> {
    if declares_encryption(out) {
        return Err(CommandError::PdfError(
            "the unlocked copy still declares encryption, so it was not written.".into(),
        ));
    }
    Ok(())
}

/// Turn a failure to *load* an encrypted document into something actionable.
///
/// The parse runs before any password we were given is tried, so a password
/// error from it is never about what the user typed — reporting the library's
/// "the supplied password is incorrect" would be actively misleading. lopdf
/// 0.36 raised one for every permissions-only file (it tried the empty
/// password and gave up); 0.42 opens those instead. The mapping stays for any
/// handler 0.42 still fails on.
fn explain_load_failure(err: &lopdf::Error) -> CommandError {
    let text = err.to_string();
    if text.contains("password") {
        return CommandError::InvalidInput(UNREADABLE.into());
    }
    CommandError::PdfError(format!("could not read the PDF: {text}"))
}

/// Catch the AES-256 variant `lopdf` cannot open, and say so plainly.
///
/// Its decrypt derives the key length as `/Length / 8` and rejects anything
/// over 16 bytes, because the legacy path it shares is MD5-based. A V5 document
/// that also carries `/Length 256` — pypdf and pyHanko write one, and we
/// briefly did too — therefore fails with `InvalidKeyLength`, which tells the
/// user nothing and reads exactly like a wrong password.
///
/// Detecting it here costs one dictionary lookup and turns an inscrutable
/// failure into an accurate one. The check is deliberately narrow: only V5 with
/// an over-long `/Length`, so a future `lopdf` that handles it correctly simply
/// stops tripping this.
fn reject_unsupported_variant(doc: &Document) -> Result<(), CommandError> {
    let Ok(dict) = doc.get_encrypted() else {
        return Ok(());
    };
    let v = dict.get(b"V").and_then(Object::as_i64).unwrap_or(0);
    let length = dict.get(b"Length").and_then(Object::as_i64).unwrap_or(0);

    if v >= 5 && length > 128 {
        return Err(CommandError::InvalidInput(
            "This file uses an AES-256 variant VibePDF can't unlock yet. \
             Documents protected by VibePDF itself will open."
                .into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // lopdf 0.42 no longer raises a password error while parsing — it opens a
    // permissions-only file instead — but the mapping is still reachable from
    // a handler it fails on, so the message has to be right: it must never
    // blame the password the user typed, because none has been tried yet.
    #[test]
    fn a_load_time_password_failure_names_the_limitation() {
        let err = explain_load_failure(&lopdf::Error::Decryption(
            lopdf::encryption::DecryptionError::IncorrectPassword,
        ));
        let CommandError::InvalidInput(msg) = &err else { panic!("got {err:?}") };
        assert_eq!(msg, UNREADABLE, "should name the limitation, not blame the password");
    }

    #[test]
    fn other_load_failures_are_reported_as_themselves() {
        let err = explain_load_failure(&lopdf::Error::CharacterEncoding);
        assert!(matches!(err, CommandError::PdfError(_)), "got {err:?}");
    }

    // The last line of defence for permissions-only files: a copy that still
    // declares encryption opens with no password exactly as the original did.
    #[test]
    fn a_copy_that_still_declares_encryption_is_refused() {
        let still = b"%PDF-1.7\ntrailer\n<< /Root 1 0 R /Encrypt 5 0 R >>\n%%EOF\n";
        let result = refuse_still_encrypted(still);
        let Err(CommandError::PdfError(msg)) = &result else { panic!("got {result:?}") };
        assert!(msg.contains("still declares encryption"), "{msg}");

        let plain = b"%PDF-1.7\ntrailer\n<< /Root 1 0 R >>\n%%EOF\n";
        assert!(refuse_still_encrypted(plain).is_ok());
    }
}
