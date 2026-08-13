//! SPEC: P6-SEC-008 (P6.C2) — remove password protection from a PDF.
//!
//! The mirror of `encrypt.rs`, and like it, no cryptography of our own:
//! `lopdf`'s `Document::decrypt` authenticates the password, decrypts every
//! string and stream, and removes both the `/Encrypt` trailer entry and the
//! encryption dictionary itself. What is left is an ordinary document.
//!
//! **On "SHALL require the owner password".** This matches the spec, though not
//! by design: for AES-256 (R6) `lopdf` authenticates the *owner* password and
//! rejects the user password. Measured on documents this project wrote —
//! a file with user `pw` and owner `owner-only` decrypts with `owner-only` and
//! refuses `pw`. So "the owner password is required" is enforced by the library,
//! not by a check here.
//!
//! Two consequences worth knowing before reading further:
//!
//! - A document with **no user password** (owner-only) cannot even be loaded:
//!   `Document::load_mem` attempts the empty user password eagerly and fails.
//!   `explain_load_failure` turns that into something a person can act on.
//! - For older handlers (RC4, AES-128) the user password works normally; the
//!   restriction is specific to R6.

use lopdf::{Document, Object};

use crate::error::CommandError;

/// SPEC: P6-SEC-008 — return `bytes` with its encryption removed.
///
/// `password` is the **owner** password for AES-256 documents (see the module
/// note); for older handlers either works. The input must actually be
/// encrypted — a plain document is refused rather than silently rewritten,
/// because "removed protection from a file that had none" and "removed
/// protection" are indistinguishable afterwards.
pub fn remove_protection(bytes: &[u8], password: &str) -> Result<Vec<u8>, CommandError> {
    let mut doc = Document::load_mem(bytes).map_err(|e| explain_load_failure(&e))?;

    if !doc.is_encrypted() {
        return Err(CommandError::InvalidInput(
            "This PDF is not password protected.".into(),
        ));
    }

    reject_unsupported_variant(&doc)?;

    doc.decrypt(password).map_err(|e| {
        // Everything here is a wrong password as far as the user is concerned;
        // the library distinguishes cases we cannot act on differently, and the
        // password itself must never appear in the message.
        tracing::debug!(error = ?e, "decrypt failed");
        CommandError::InvalidInput("That password did not unlock the document.".into())
    })?;

    let mut out = Vec::new();
    doc.save_to(&mut out)
        .map_err(|e| CommandError::PdfError(format!("could not write the unlocked PDF: {e}")))?;
    Ok(out)
}

/// Turn a failure to *load* an encrypted document into something actionable.
///
/// `Document::load_mem` tries the empty user password as it parses, so a
/// document with an owner password and no user password fails here rather than
/// at `decrypt` — before any password we were given has been tried. Reporting
/// the library's "the supplied password is incorrect" would be actively
/// misleading: the password the user typed was never consulted.
fn explain_load_failure(err: &lopdf::Error) -> CommandError {
    let text = err.to_string();
    if text.contains("password") {
        return CommandError::InvalidInput(
            "VibePDF can't unlock this document. Files protected only by a permissions \
             password (with no password to open) aren't supported yet."
                .into(),
        );
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
