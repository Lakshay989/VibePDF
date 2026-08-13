//! The signature container: `/Sig` dict, `/ByteRange`, and the `/Contents`
//! placeholder (P6.B1a, part one of SPEC P6-SEC-005).
//!
//! **No cryptography here, and none coming.** This module builds the *hole* a
//! signature goes into and works out exactly which bytes a signature covers.
//! Producing the CMS/PKCS#7 blob that fills the hole is a separate concern with
//! a separate set of dependencies; this file never sees a key.
//!
//! ## Why a placeholder at all
//!
//! A PDF signature signs the file it lives in, which is circular: the signature
//! bytes cannot be part of what they sign. PDF 32000-1 §12.8.1 resolves it by
//! having `/Contents` — the signature itself — sit in a gap that `/ByteRange`
//! declares excluded. So the sequence is fixed and cannot be reordered:
//!
//! 1. write the signature dictionary with `/Contents` reserved as zeros;
//! 2. serialise the whole file, because only then do byte offsets exist;
//! 3. find the gap, write the real `/ByteRange` **without changing the file's
//!    length** — the offsets would move if it did;
//! 4. hash everything outside the gap;
//! 5. drop the signature into the gap.
//!
//! Step 3 is where this goes wrong quietly. `/ByteRange` is itself inside the
//! signed region, so patching it after hashing produces a file whose signature
//! is invalid in a way that looks like tampering. The API here makes that
//! ordering impossible to get wrong: [`PreparedSignature::message`] can only be
//! called on an already-patched buffer.
//!
//! ## Why an incremental update
//!
//! Signing **appends**; it does not rewrite. The original bytes survive
//! verbatim as a prefix of the output (`the_original_bytes_are_untouched`
//! pins this). Rewriting would invalidate any signature already on the
//! document, and `PAdES` requires each signature to cover a real earlier state of
//! the file rather than a re-serialisation of it.

use std::ops::Range;

use lopdf::{Dictionary, Document, IncrementalDocument, Object, ObjectId, StringFormat};

use crate::error::CommandError;

/// Bytes reserved for the signature blob. The hex string in the file is twice
/// this.
///
/// A CMS `SignedData` with one RSA-2048 signature and a three-certificate chain
/// runs about 4 KB; timestamps and revocation data (`PAdES` B-LT) push it higher.
/// 16 KB is the value Acrobat and most signing libraries reserve, and the cost
/// of being generous is 32 KB of zeros in a file that is already going to be
/// re-saved. The cost of being stingy is a signature that will not fit, found
/// only at the last step.
const PLACEHOLDER_BYTES: usize = 16 * 1024;

/// Flags for the signature widget: bit 3 (Print, 4) so it appears in print, and
/// bit 8 (`Locked`, 128) so the field cannot be moved or deleted in a viewer.
const WIDGET_FLAGS: i64 = 132;

/// `/SigFlags`: bit 1 `SignaturesExist`, bit 2 `AppendOnly`. Both, per §12.7.2 —
/// `AppendOnly` tells a viewer this document must only ever be updated
/// incrementally, which is the invariant signatures depend on.
const SIG_FLAGS: i64 = 3;

#[allow(clippy::needless_pass_by_value)]
fn sign_err(e: lopdf::Error) -> CommandError {
    CommandError::PdfError(format!("sign: {e}"))
}

/// What goes in the signature dictionary besides the signature.
///
/// `signed_at` is supplied by the caller rather than read from the clock here:
/// it has to match the signing time inside the CMS blob, it is the one input
/// that would make these tests non-deterministic, and formatting a PDF date
/// would otherwise mean a date library this crate does not have.
#[derive(Debug, Clone, Default)]
pub struct SignatureSpec {
    /// Field name, e.g. `Signature1`.
    pub field_name: String,
    /// PDF date string: `D:YYYYMMDDHHmmSS+HH'mm'`.
    pub signed_at: String,
    pub reason: Option<String>,
    pub location: Option<String>,
    pub contact: Option<String>,
    /// Human-readable signer name (`/Name`). Not a security claim — the
    /// certificate is what actually says who signed.
    pub name: Option<String>,
}

/// The parts of a signature a user fills in, as they arrive over IPC.
///
/// Separate from [`SignatureSpec`] because the field name is ours to choose,
/// not the user's — and because a command taking nine positional arguments is
/// one transposition away from sending a password where a path belongs.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureDetails {
    /// PDF date string: `D:YYYYMMDDHHmmSS+HH'mm'`.
    pub signed_at: String,
    pub reason: Option<String>,
    pub location: Option<String>,
    pub name: Option<String>,
}

impl SignatureDetails {
    /// Fill in the field name and become a [`SignatureSpec`].
    #[must_use]
    pub fn into_spec(self, field_name: &str) -> SignatureSpec {
        SignatureSpec {
            field_name: field_name.to_owned(),
            signed_at: self.signed_at,
            reason: self.reason,
            location: self.location,
            contact: None,
            name: self.name,
        }
    }
}

/// A document with a signature-shaped hole in it, ready to be hashed.
///
/// `Debug` deliberately omits `bytes`: printing a whole PDF into a test failure
/// or a log helps nobody.
pub struct PreparedSignature {
    bytes: Vec<u8>,
    byte_range: [usize; 4],
    /// The `<…>` span, inclusive of both delimiters — the bytes `/ByteRange`
    /// declares excluded.
    hole: Range<usize>,
}

impl std::fmt::Debug for PreparedSignature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedSignature")
            .field("len", &self.bytes.len())
            .field("byte_range", &self.byte_range)
            .field("hole", &self.hole)
            .finish()
    }
}

impl PreparedSignature {
    /// The bytes the signature covers: everything outside the hole.
    ///
    /// This is what gets hashed. It is deliberately a copy rather than two
    /// slices — a caller that hashed only the first range would produce a
    /// signature that verifies against nothing, and the shape of the API
    /// should not make that convenient.
    #[must_use]
    pub fn message(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.bytes.len() - self.hole.len());
        out.extend_from_slice(&self.bytes[..self.hole.start]);
        out.extend_from_slice(&self.bytes[self.hole.end..]);
        out
    }

    /// The `/ByteRange` as written into the file.
    #[must_use]
    pub fn byte_range(&self) -> [usize; 4] {
        self.byte_range
    }

    /// Largest signature, in bytes, that [`Self::embed`] will accept.
    #[must_use]
    pub fn capacity(&self) -> usize {
        (self.hole.len() - 2) / 2
    }

    /// Drop the DER-encoded signature into the hole.
    ///
    /// The hole is a fixed size, so a short signature is padded with zeros
    /// rather than shrinking the gap — moving any byte after it would
    /// invalidate every offset the signature just committed to.
    pub fn embed(mut self, der: &[u8]) -> Result<Vec<u8>, CommandError> {
        if der.len() > self.capacity() {
            return Err(CommandError::Internal(format!(
                "signature is {} bytes but only {} are reserved; \
                 raise PLACEHOLDER_BYTES",
                der.len(),
                self.capacity()
            )));
        }
        // Skip the '<'; write hex; leave the rest of the run as the '0's it
        // already contains, which decode to trailing zero bytes a verifier
        // ignores.
        let mut at = self.hole.start + 1;
        for byte in der {
            let hi = HEX[usize::from(byte >> 4)];
            let lo = HEX[usize::from(byte & 0x0f)];
            self.bytes[at] = hi;
            self.bytes[at + 1] = lo;
            at += 2;
        }
        Ok(self.bytes)
    }
}

const HEX: [u8; 16] = *b"0123456789ABCDEF";

/// SPEC: P6-SEC-005 — append a signature field to `bytes` and reserve the gap.
///
/// The result is a complete, openable PDF whose signature is present but empty.
/// That is a real state, not a broken one: a viewer shows an unsigned signature
/// field. Nothing here is irreversible, which is what makes it testable on its
/// own.
pub fn prepare(bytes: &[u8], spec: &SignatureSpec) -> Result<PreparedSignature, CommandError> {
    let prev = Document::load_mem(bytes).map_err(sign_err)?;
    let mut doc = IncrementalDocument::create_from(bytes.to_vec(), prev);

    let page_id = first_page_id(doc.get_prev_documents())?;
    let sig_id = add_signature_dict(&mut doc, spec);
    let widget_id = add_widget(&mut doc, spec, sig_id, page_id);
    attach_to_page(&mut doc, page_id, widget_id)?;
    register_in_acroform(&mut doc, widget_id)?;

    let mut out = Vec::new();
    doc.save_to(&mut out)
        .map_err(|e| CommandError::PdfError(format!("sign: lopdf save: {e}")))?;

    locate_and_patch(out)
}

/// The first page, from the *previous* document — the incremental update has no
/// page tree of its own until we clone one in.
fn first_page_id(prev: &Document) -> Result<ObjectId, CommandError> {
    prev.get_pages()
        .into_values()
        .next()
        .ok_or_else(|| CommandError::InvalidInput("the document has no pages".into()))
}

fn text_entry(dict: &mut Dictionary, key: &str, value: Option<&String>) {
    if let Some(v) = value {
        dict.set(key, Object::String(v.as_bytes().to_vec(), StringFormat::Literal));
    }
}

/// The `/Sig` dictionary, with `/Contents` reserved and `/ByteRange` a
/// placeholder wide enough for any offset we could later need.
fn add_signature_dict(doc: &mut IncrementalDocument, spec: &SignatureSpec) -> ObjectId {
    let mut sig = Dictionary::new();
    sig.set("Type", Object::Name(b"Sig".to_vec()));
    sig.set("Filter", Object::Name(b"Adobe.PPKLite".to_vec()));
    // PAdES (ETSI EN 319 142) rather than the older /adbe.pkcs7.detached. The
    // blob this reserves space for is a CAdES SignedData either way; the name
    // is what tells a verifier which profile's rules to apply.
    sig.set("SubFilter", Object::Name(b"ETSI.CAdES.detached".to_vec()));

    // Ten digits each: enough for a 9.9 GB file, and fixed-width so the real
    // offsets can be written over them without moving a byte.
    sig.set(
        "ByteRange",
        Object::Array(vec![
            Object::Integer(0),
            Object::Integer(9_999_999_999),
            Object::Integer(9_999_999_999),
            Object::Integer(9_999_999_999),
        ]),
    );
    sig.set(
        "Contents",
        Object::String(vec![0u8; PLACEHOLDER_BYTES], StringFormat::Hexadecimal),
    );
    sig.set(
        "M",
        Object::String(spec.signed_at.as_bytes().to_vec(), StringFormat::Literal),
    );
    text_entry(&mut sig, "Reason", spec.reason.as_ref());
    text_entry(&mut sig, "Location", spec.location.as_ref());
    text_entry(&mut sig, "ContactInfo", spec.contact.as_ref());
    text_entry(&mut sig, "Name", spec.name.as_ref());

    doc.new_document.add_object(sig)
}

/// The widget annotation that is also the form field — the single-widget shape,
/// which is what every producer emits for a lone signature.
///
/// `/Rect [0 0 0 0]` makes it invisible. A drawn appearance is P6.A5b's
/// business: a picture of a signature and a cryptographic signature are
/// different claims, and this one is the real claim.
fn add_widget(
    doc: &mut IncrementalDocument,
    spec: &SignatureSpec,
    sig_id: ObjectId,
    page_id: ObjectId,
) -> ObjectId {
    let mut widget = Dictionary::new();
    widget.set("Type", Object::Name(b"Annot".to_vec()));
    widget.set("Subtype", Object::Name(b"Widget".to_vec()));
    widget.set("FT", Object::Name(b"Sig".to_vec()));
    widget.set(
        "T",
        Object::String(spec.field_name.as_bytes().to_vec(), StringFormat::Literal),
    );
    widget.set("V", Object::Reference(sig_id));
    widget.set(
        "Rect",
        Object::Array(vec![
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(0),
        ]),
    );
    widget.set("F", Object::Integer(WIDGET_FLAGS));
    widget.set("P", Object::Reference(page_id));
    doc.new_document.add_object(widget)
}

/// Add the widget to the page's `/Annots`, carrying over what was already there.
fn attach_to_page(
    doc: &mut IncrementalDocument,
    page_id: ObjectId,
    widget_id: ObjectId,
) -> Result<(), CommandError> {
    // The page must be copied into the update before it can be changed; the
    // previous revision's bytes stay exactly as they are.
    doc.opt_clone_object_to_new_document(page_id)
        .map_err(sign_err)?;

    let existing: Vec<Object> = doc
        .new_document
        .get_object(page_id)
        .and_then(Object::as_dict)
        .ok()
        .and_then(|d| d.get(b"Annots").ok())
        .and_then(|o| match o {
            Object::Array(a) => Some(a.clone()),
            Object::Reference(r) => doc
                .get_prev_documents()
                .get_object(*r)
                .and_then(Object::as_array)
                .ok()
                .cloned(),
            _ => None,
        })
        .unwrap_or_default();

    let mut annots = existing;
    annots.push(Object::Reference(widget_id));

    let page = doc
        .new_document
        .get_object_mut(page_id)
        .and_then(Object::as_dict_mut)
        .map_err(sign_err)?;
    page.set("Annots", Object::Array(annots));
    Ok(())
}

/// Put the field in `/AcroForm /Fields` and set `/SigFlags`.
///
/// A signature the form does not list is one many viewers will not show at all,
/// even though the annotation is on the page.
fn register_in_acroform(
    doc: &mut IncrementalDocument,
    widget_id: ObjectId,
) -> Result<(), CommandError> {
    let root = doc
        .get_prev_documents()
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .map_err(|e| CommandError::PdfError(format!("sign: no /Root: {e}")))?;
    doc.opt_clone_object_to_new_document(root).map_err(sign_err)?;

    // An existing AcroForm is an indirect object more often than not; either
    // way its fields have to come along or the document loses its form.
    let acro_ref = doc
        .new_document
        .get_object(root)
        .and_then(Object::as_dict)
        .ok()
        .and_then(|d| d.get(b"AcroForm").ok())
        .and_then(|o| o.as_reference().ok());

    let mut acro = match acro_ref {
        Some(id) => doc
            .get_prev_documents()
            .get_object(id)
            .and_then(Object::as_dict)
            .cloned()
            .unwrap_or_default(),
        None => doc
            .new_document
            .get_object(root)
            .and_then(Object::as_dict)
            .ok()
            .and_then(|d| d.get(b"AcroForm").ok())
            .and_then(|o| o.as_dict().ok())
            .cloned()
            .unwrap_or_default(),
    };

    let mut fields: Vec<Object> = acro
        .get(b"Fields")
        .ok()
        .and_then(|o| match o {
            Object::Array(a) => Some(a.clone()),
            Object::Reference(r) => doc
                .get_prev_documents()
                .get_object(*r)
                .and_then(Object::as_array)
                .ok()
                .cloned(),
            _ => None,
        })
        .unwrap_or_default();
    fields.push(Object::Reference(widget_id));

    acro.set("Fields", Object::Array(fields));
    acro.set("SigFlags", Object::Integer(SIG_FLAGS));

    let acro_id = doc.new_document.add_object(Object::Dictionary(acro));
    let catalog = doc
        .new_document
        .get_object_mut(root)
        .and_then(Object::as_dict_mut)
        .map_err(sign_err)?;
    catalog.set("AcroForm", Object::Reference(acro_id));
    Ok(())
}

/// Find the reserved gap, write the real `/ByteRange` over the placeholder, and
/// return the result.
///
/// The patch must not change the file's length. `/ByteRange` sits *before* the
/// gap, so every offset it names would shift if the array grew or shrank — and
/// the file would then declare offsets that are wrong by exactly the amount it
/// moved them. Numbers are written left-aligned into their ten-character slots
/// and padded with spaces, which PDF treats as ordinary separators.
fn locate_and_patch(mut bytes: Vec<u8>) -> Result<PreparedSignature, CommandError> {
    // The placeholder is 32 KB of '0' between angle brackets. Nothing else in a
    // PDF looks like that, which is why the gap is found by its contents rather
    // than by searching for `/Contents` — pages have a `/Contents` too.
    let run = PLACEHOLDER_BYTES * 2;
    let start = find_placeholder(&bytes, run).ok_or_else(|| {
        CommandError::Internal("sign: the /Contents placeholder was not in the saved file".into())
    })?;
    let hole = start..start + run + 2; // '<' + hex + '>'

    let total = bytes.len();
    let byte_range = [0usize, hole.start, hole.end, total - hole.end];

    let array = find_byte_range_array(&bytes)?;
    let mut patched = format!(
        "[0 {} {} {}]",
        byte_range[1], byte_range[2], byte_range[3]
    )
    .into_bytes();
    if patched.len() > array.len() {
        return Err(CommandError::Internal(
            "sign: the real /ByteRange is wider than its placeholder".into(),
        ));
    }
    // Pad *inside* the brackets so the array keeps its exact width.
    patched.pop();
    while patched.len() < array.len() - 1 {
        patched.push(b' ');
    }
    patched.push(b']');
    bytes[array.clone()].copy_from_slice(&patched);

    debug_assert_eq!(bytes.len(), total, "patching moved the file");
    Ok(PreparedSignature {
        bytes,
        byte_range,
        hole,
    })
}

/// Offset of the `<` opening a run of exactly `run` zero digits.
fn find_placeholder(bytes: &[u8], run: usize) -> Option<usize> {
    let mut zeros = 0usize;
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'0' {
            zeros += 1;
            continue;
        }
        if *b == b'>' && zeros >= run {
            // Walk back over the run to its '<'.
            let open = i - zeros - 1;
            if bytes.get(open) == Some(&b'<') {
                return Some(open);
            }
        }
        zeros = 0;
    }
    None
}

/// The `[...]` following the one `/ByteRange` in the file.
fn find_byte_range_array(bytes: &[u8]) -> Result<Range<usize>, CommandError> {
    let key = b"/ByteRange";
    let mut found = None;
    for i in 0..bytes.len().saturating_sub(key.len()) {
        if &bytes[i..i + key.len()] == key {
            if found.is_some() {
                return Err(CommandError::Internal(
                    "sign: more than one /ByteRange; signing an already-signed \
                     document is not supported yet"
                        .into(),
                ));
            }
            found = Some(i);
        }
    }
    let at = found
        .ok_or_else(|| CommandError::Internal("sign: no /ByteRange in the saved file".into()))?;

    let open = bytes[at..]
        .iter()
        .position(|b| *b == b'[')
        .ok_or_else(|| CommandError::Internal("sign: /ByteRange has no array".into()))?
        + at;
    let close = bytes[open..]
        .iter()
        .position(|b| *b == b']')
        .ok_or_else(|| CommandError::Internal("sign: /ByteRange array is unterminated".into()))?
        + open;
    Ok(open..close + 1)
}

/// SPEC: P6-SEC-005 — sign `bytes` with the certificate in `pfx`.
///
/// The whole sequence in one place, because the order is the correctness
/// argument and splitting it across callers would let someone hash before
/// patching:
///
/// 1. [`prepare`] appends the field and reserves the gap;
/// 2. `/ByteRange` is already written, so the message is final;
/// 3. the CMS blob is built over that message;
/// 4. [`PreparedSignature::embed`] drops it into the gap.
///
/// The password never leaves this frame and never enters an error.
pub fn sign_document(
    bytes: &[u8],
    spec: &SignatureSpec,
    pfx: &[u8],
    password: &str,
) -> Result<Vec<u8>, CommandError> {
    let credential = crate::security::credential::load_pkcs12(pfx, password)?;
    let prepared = prepare(bytes, spec)?;
    let blob = crate::security::cms::sign_detached(&prepared.message(), &credential)?;
    prepared.embed(&blob)
}
