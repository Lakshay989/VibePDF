//! SPEC: P6-SEC-008 (P6.C2) — remove password protection from a PDF.
//!
//! The mirror of `encrypt.rs`, and like it, no cryptography of our own:
//! `lopdf` authenticates the password, decrypts every string and stream, and
//! drops both the `/Encrypt` trailer entry and the encryption dictionary. What
//! is left is an ordinary document.
//!
//! **"SHALL require the owner password" is checked here, for every handler.**
//! lopdf's loader accepts either password — it tries the owner check, then the
//! user one — so it cannot be what enforces the spec. Up to lopdf 0.36 the
//! AES-256 path happened to refuse the user password while RC4 and AES-128
//! accepted it; since lopdf 0.42 (2026-10-17) the supplied password is
//! authenticated *as the owner password* before anything is decrypted.
//!
//! That check needs the encryption dictionary, and lopdf discards it whenever
//! it decrypts on load. A file that needs an open password keeps it: parsed
//! without a password, it comes back as its trailer and that dictionary and
//! nothing else. A **permissions-only** file (no open password) does not —
//! lopdf opens it with the empty password and throws the dictionary away — so
//! its owner password cannot be checked, and it is refused rather than
//! unlocked by any password at all. That is a limitation, and says so.
//!
//! This file parses with lopdf directly rather than through
//! `pdf::document::load_cos`, because the encrypted, undecrypted document is
//! exactly what `load_cos` refuses and exactly what the check needs.
//! `tests/lopdf_load_policy.rs` allows it by name.

use lopdf::{Document, LoadOptions, Object};

use crate::error::CommandError;
use crate::pdf::document::declares_encryption;

/// Why a permissions-only document is refused (see the module note).
const PERMISSIONS_ONLY: &str = "VibePDF can't unlock this document. Files protected only by a \
    permissions password (with no password to open) aren't supported yet.";

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
    // could be saved by mistake.
    let locked = Document::load_mem(bytes).map_err(|e| explain_load_failure(&e))?;
    if !locked.is_encrypted() {
        // Decrypted with the empty open password, dictionary gone: the owner
        // password can no longer be checked, so nothing may unlock it.
        return Err(CommandError::InvalidInput(PERMISSIONS_ONLY.into()));
    }
    reject_unsupported_variant(&locked)?;
    require_owner_password(&locked, password)?;

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
    Ok(out)
}

/// The supplied password, authenticated as the **owner** password.
///
/// A user password that would open the document is named as the wrong kind,
/// since "did not unlock" would read as a typo to someone holding the right
/// password for a different purpose. The password never appears in a message.
fn require_owner_password(locked: &Document, password: &str) -> Result<(), CommandError> {
    if locked.authenticate_owner_password(password).is_ok() {
        return Ok(());
    }
    if locked.authenticate_user_password(password).is_ok() {
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

/// Turn a failure to *load* an encrypted document into something actionable.
///
/// The parse runs before any password we were given is tried, so a password
/// error from it is never about what the user typed — reporting the library's
/// "the supplied password is incorrect" would be actively misleading. lopdf
/// 0.36 raised one for every permissions-only file (it tried the empty
/// password and gave up); 0.42 opens those instead, and the module note covers
/// what happens then. The mapping stays for any handler 0.42 still fails on.
fn explain_load_failure(err: &lopdf::Error) -> CommandError {
    let text = err.to_string();
    if text.contains("password") {
        return CommandError::InvalidInput(PERMISSIONS_ONLY.into());
    }
    CommandError::PdfError(format!("could not read the PDF: {text}"))
}

/// Catch the AES-256 variant `lopdf` cannot open, and say so plainly.
///
/// Its decrypt derives the key length as `/Length / 8` and rejects anything
/// over 16 bytes, because the legacy path it shares is MD5-based. A V5 document
/// that also carries `/Length 256` — pypdf writes one, and we briefly did too —
/// therefore fails with `InvalidKeyLength`, which tells the user nothing and
/// reads exactly like a wrong password.
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

    // This path is no longer reachable from our own output — P6.C1 refuses to
    // write owner-only documents precisely because of it — but it is still
    // reachable with such a file from another tool, so the message has to be
    // right. Unit-tested here because building one to order would mean
    // reimplementing the encrypt path with its `/Perms` workaround.
    #[test]
    fn a_load_time_password_failure_names_the_limitation() {
        let err = explain_load_failure(&lopdf::Error::Decryption(
            lopdf::encryption::DecryptionError::IncorrectPassword,
        ));
        let CommandError::InvalidInput(msg) = &err else { panic!("got {err:?}") };
        assert!(
            msg.contains("aren't supported yet"),
            "should name the limitation, not blame the password the user typed: {msg}"
        );
    }

    #[test]
    fn other_load_failures_are_reported_as_themselves() {
        let err = explain_load_failure(&lopdf::Error::CharacterEncoding);
        assert!(matches!(err, CommandError::PdfError(_)), "got {err:?}");
    }
}
