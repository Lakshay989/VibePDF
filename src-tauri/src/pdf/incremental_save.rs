//! SPEC: P6-SEC-006 — saving a signed document must not destroy its signatures.
//!
//! A signature covers exact byte ranges of the file as it was when signed. Every
//! save used to rewrite the whole file through `PDFium`, which moves those
//! bytes, so any save — even of an edit that was then undone — left a signature
//! no reader could check. The P6 sweep found it as "The signature covers bytes
//! outside the file."
//!
//! The fix is what mainstream editors do: leave the signed bytes exactly where
//! they are and append the changes as an incremental update. The signature
//! still verifies for the revision it signed, and a reader reports that the
//! document changed afterwards — which is the truth.
//!
//! `PDFium` still makes every edit. This module only decides which objects
//! changed, by comparing its full rewrite with the signed file object by object,
//! and appends those. That relies on `PDFium` keeping object numbers across a
//! rewrite and giving new objects fresh ones — measured on signed and form
//! documents, not assumed: adding a note changes the page dictionary and adds
//! one object; undoing it returns every object to identical. The one thing a
//! rewrite changes without an edit is stream encoding, which is why streams are
//! compared by decoded content.

use std::path::Path;

use lopdf::{Dictionary, Document, IncrementalDocument, Object, Stream};
use pdfium_render::prelude::PdfDocument;

use crate::error::CommandError;

/// The bytes a signed document was opened from, or `None` for an unsigned one.
///
/// Unsigned documents keep being saved whole, exactly as before; only a
/// document `PDFium` reports signatures in pays for the extra copy of its bytes.
/// If the file can no longer be read, saving falls back to a full rewrite — the
/// same behaviour as before this module existed, logged rather than hidden.
pub fn signed_base_for(doc: &PdfDocument<'_>, path: &Path) -> Option<Vec<u8>> {
    let signed = crate::pdf::document::pdfium_lock()
        .is_ok_and(|_guard| !doc.signatures().is_empty());
    if !signed {
        return None;
    }
    match std::fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(e) => {
            tracing::warn!(error = %e, "signed document unreadable; saves will rewrite it whole");
            None
        }
    }
}

/// `rewrite` — `PDFium`'s full serialisation of the edited document — expressed
/// as an incremental update appended to `base`, the signed file.
///
/// The result begins with every byte of `base`, unchanged. That is checked
/// before returning, as is that every object of `rewrite` reads back from the
/// result: a save that silently dropped an edit, or moved a signed byte, is
/// worse than one that refuses.
pub fn append_as_incremental_update(base: &[u8], rewrite: &[u8]) -> Result<Vec<u8>, CommandError> {
    let prev = Document::load_mem(base).map_err(pdf_err("read the signed file"))?;
    let edited = Document::load_mem(rewrite).map_err(pdf_err("read the edited document"))?;

    // Appending to an encrypted file means encrypting each appended object with
    // the document's key, which nothing here can do yet. Refusing keeps the
    // user's edits in the open document; rewriting would quietly break the
    // signature, which is the failure this module exists to prevent.
    if prev.is_encrypted() || edited.is_encrypted() {
        return Err(CommandError::InvalidInput(
            "This document is both signed and password protected, and VibePDF can't save \
             changes to it without breaking the signature yet. Remove the protection first, \
             or close without saving."
                .into(),
        ));
    }

    let mut update = IncrementalDocument::create_from(base.to_vec(), prev.clone());
    for (id, object) in &edited.objects {
        if prev.objects.get(id).is_some_and(|old| equivalent(old, object)) {
            continue;
        }
        update.new_document.objects.insert(*id, object.clone());
    }
    if update.new_document.objects.is_empty() {
        // Nothing actually changed — an edit and its undo. Appending an empty
        // revision would still tell every reader "modified after signing".
        return Ok(base.to_vec());
    }

    update.new_document.max_id = update.new_document.max_id.max(edited.max_id);
    for key in [&b"Root"[..], b"Info", b"ID"] {
        if let Ok(value) = edited.trailer.get(key) {
            update.new_document.trailer.set(key, value.clone());
        }
    }

    let mut out = Vec::new();
    update
        .save_to(&mut out)
        .map_err(|e| CommandError::PdfError(format!("could not append the changes: {e}")))?;

    if !out.starts_with(base) {
        return Err(CommandError::Internal("saving would have moved signed bytes".into()));
    }
    let reread = Document::load_mem(&out).map_err(pdf_err("read the saved document back"))?;
    for (id, object) in &edited.objects {
        if !reread.objects.get(id).is_some_and(|saved| equivalent(saved, object)) {
            return Err(CommandError::Internal(format!(
                "object {} {} did not survive saving",
                id.0, id.1
            )));
        }
    }
    if reread.get_pages().len() != edited.get_pages().len() {
        return Err(CommandError::Internal("saving changed the page count".into()));
    }
    Ok(out)
}

fn pdf_err(context: &'static str) -> impl Fn(lopdf::Error) -> CommandError {
    move |e| CommandError::PdfError(format!("could not {context}: {e}"))
}

/// Keys that describe how a stream is *stored*, not what it contains.
const STREAM_ENCODING_KEYS: &[&[u8]] = &[b"Length", b"Filter", b"DecodeParms"];

/// Whether two objects mean the same thing.
///
/// Deliberately narrow in what it forgives. Everything forgiven here is a
/// spelling difference with no meaning — dictionary key order, literal versus
/// hex string, how a stream is compressed — because anything wrongly called
/// "the same" is an edit that never reaches the saved file.
fn equivalent(a: &Object, b: &Object) -> bool {
    match (a, b) {
        (Object::String(x, _), Object::String(y, _)) => x == y,
        (Object::Array(x), Object::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| equivalent(p, q))
        }
        (Object::Dictionary(x), Object::Dictionary(y)) => dictionaries_equivalent(x, y, &[]),
        (Object::Stream(x), Object::Stream(y)) => {
            dictionaries_equivalent(&x.dict, &y.dict, STREAM_ENCODING_KEYS) && decoded(x) == decoded(y)
        }
        _ => a == b,
    }
}

fn dictionaries_equivalent(x: &Dictionary, y: &Dictionary, ignoring: &[&[u8]]) -> bool {
    let kept = |d: &Dictionary| d.iter().filter(|(k, _)| !ignoring.contains(&k.as_slice())).count();
    kept(x) == kept(y)
        && x.iter()
            .filter(|(k, _)| !ignoring.contains(&k.as_slice()))
            .all(|(k, v)| y.get(k).is_ok_and(|w| equivalent(v, w)))
}

fn decoded(stream: &Stream) -> Vec<u8> {
    stream.decompressed_content().unwrap_or_else(|_| stream.content.clone())
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;
    use lopdf::{dictionary, StringFormat};

    fn stream(content: &[u8], compress: bool) -> Object {
        let mut s = Stream::new(dictionary! {}, content.to_vec());
        if compress {
            s.compress().expect("compress");
        }
        Object::Stream(s)
    }

    // What PDFium does to a content stream on every save, edit or not.
    #[test]
    fn a_recompressed_stream_is_the_same_object() {
        assert!(equivalent(&stream(b"BT /F1 24 Tf (Hello) Tj ET", false), &stream(b"BT /F1 24 Tf (Hello) Tj ET", true)));
    }

    #[test]
    fn a_stream_with_different_content_is_not() {
        assert!(!equivalent(&stream(b"BT (Hello) Tj ET", true), &stream(b"BT (Hellp) Tj ET", true)));
    }

    #[test]
    fn string_spelling_is_forgiven_but_string_content_is_not() {
        let lit = Object::String(b"abc".to_vec(), StringFormat::Literal);
        let hex = Object::String(b"abc".to_vec(), StringFormat::Hexadecimal);
        let other = Object::String(b"abd".to_vec(), StringFormat::Literal);
        assert!(equivalent(&lit, &hex));
        assert!(!equivalent(&lit, &other));
    }

    // An edit usually lands somewhere deep — one entry in a page's /Annots.
    #[test]
    fn a_change_deep_inside_a_dictionary_is_noticed() {
        let page = |annots: Vec<Object>| Object::Dictionary(dictionary! { "Type" => "Page", "Annots" => annots });
        let before = page(vec![Object::Reference((7, 0))]);
        let after = page(vec![Object::Reference((7, 0)), Object::Reference((9, 0))]);
        assert!(!equivalent(&before, &after));
        assert!(equivalent(&before, &page(vec![Object::Reference((7, 0))])));
    }
}
