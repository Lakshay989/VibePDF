//! Verifying the signatures on a document (P6.B2a, SPEC P6-SEC-006).
//!
//! **No cryptography is implemented here.** `rsa` checks the signature, `sha2`
//! computes the digest, `x509-cert` and `cms` parse. This file decides *what to
//! check* and, more importantly, what we are entitled to say about the answer.
//!
//! ## What a verifier actually does
//!
//! A PDF signature does not sign the document directly. It signs a set of
//! attributes, one of which is a digest of the bytes `/ByteRange` names. So
//! there are two independent checks and they fail for different reasons:
//!
//! 1. **the digest** — hash the `/ByteRange` bytes and compare with the
//!    `message-digest` attribute. A mismatch means the *document* changed.
//! 2. **the signature** — verify the RSA signature over the DER-encoded signed
//!    attributes. A failure here means the *signature* is forged or the wrong
//!    certificate is attached; the document may be untouched.
//!
//! Collapsing them into one boolean throws away the distinction a user needs,
//! so [`SignatureReport`] keeps them apart.
//!
//! ## What we cannot say
//!
//! **We have no trust anchors, so we never report a signature as trusted.**
//! Deciding that a certificate is trustworthy means checking it against a list
//! of roots someone vouches for — for document signing that is the AATL or
//! the EU trust list, neither of which is a thing we can bundle, and neither of
//! which is the TLS root store. `VibePDF` therefore reports the *shape* of the
//! chain — self-signed, or issued by someone we cannot check — and says plainly
//! that trust is unknown.
//!
//! That is the honest answer, and it is also the safe one. A verifier that
//! reports "trusted" because a chain is internally consistent is worse than one
//! that reports nothing: every self-signed certificate has an internally
//! consistent chain of one.

use cms::content_info::ContentInfo;
use cms::signed_data::{SignedData, SignerInfo};
use const_oid::db::rfc5911::{ID_MESSAGE_DIGEST, ID_SIGNED_DATA};
use const_oid::db::rfc5912::ID_SHA_256;
use der::{Decode, Encode};
use lopdf::{Document, Object, ObjectId};
use sha2::{Digest, Sha256};
use std::time::SystemTime;
use x509_cert::Certificate;

use crate::error::CommandError;
use crate::pdf::document::{declares_encryption, load_cos, load_cos_with_password};

/// How much we can say about the certificate's issuer.
///
/// Deliberately has no `Trusted` variant. See the module note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChainStatus {
    /// Issuer and subject are the same. Common for test certificates, and
    /// vouched for by nobody but itself.
    SelfSigned,
    /// Issued by someone else, and the chain in the file leads to a root we
    /// cannot check against any trust list.
    IssuerNotChecked,
    /// The file does not contain the certificates needed to follow the chain.
    Incomplete,
    /// A certificate does not verify against the key that supposedly issued it.
    /// Somebody altered a certificate, and the name shown next to this
    /// signature is not one to believe.
    Broken,
}

/// SPEC: P6-SEC-006 — the per-signature status the spec asks us to display.
///
/// `struct_excessive_bools` would have these as a state machine. They are not
/// states of one thing: a signature can be cryptographically valid *and* over a
/// changed document *and* expired, and each combination means something
/// different to a user. Collapsing them is exactly what this module exists not
/// to do.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureReport {
    /// The form field's name, when it has one.
    pub field_name: Option<String>,
    /// The signing certificate's subject, as a readable DN.
    pub signer: String,
    /// The certificate's issuer.
    pub issuer: String,
    /// `/M`, the claimed signing time. Claimed: nothing here proves it, and
    /// without a timestamp token nothing can.
    pub signed_at: Option<String>,
    pub reason: Option<String>,
    /// The RSA signature over the signed attributes checks out.
    pub signature_valid: bool,
    /// The document's bytes still hash to what was signed.
    pub digest_matches: bool,
    /// `/ByteRange` runs to the end of the file. When it does not, something
    /// was appended after signing — which may be an entirely legitimate second
    /// signature, and is still a change the reader should be told about.
    pub covers_whole_document: bool,
    /// The certificate was outside its validity window at the time we checked.
    pub certificate_expired: bool,
    pub chain: ChainStatus,
    /// `/P` from a `DocMDP` `/Reference`, when this is a certification signature.
    pub certification_level: Option<i64>,
    /// Anything that stopped a check from running, in the user's language.
    pub problems: Vec<String>,
}

impl SignatureReport {
    /// The one-line summary: is this signature good?
    ///
    /// Trust is deliberately not part of it — we cannot assess trust, and a
    /// summary that quietly ignored that would be the misleading kind of true.
    #[must_use]
    pub fn is_intact(&self) -> bool {
        self.signature_valid
            && self.digest_matches
            && self.chain != ChainStatus::Broken
            && self.problems.is_empty()
    }
}

fn pdf_err(what: &str, e: impl std::fmt::Display) -> CommandError {
    CommandError::PdfError(format!("verify: {what}: {e}"))
}

/// SPEC: P6-SEC-006 — verify every signature in `bytes`.
///
/// `now` is a parameter rather than a call to the clock so that expiry is
/// testable, and so a caller can ask "was this valid when it was signed?"
/// rather than only "is it valid today?".
///
/// An unsigned document yields an empty list — not an error. Most documents are
/// unsigned.
pub fn verify_signatures(
    bytes: &[u8],
    now: SystemTime,
) -> Result<Vec<SignatureReport>, CommandError> {
    verify_signatures_opened_with(bytes, None, now)
}

/// SPEC: P6-SEC-006 — [`verify_signatures`] for a document opened with
/// `password`, which a protected file needs.
///
/// lopdf parses a file that needs an open password as nothing but its
/// encryption dictionary, so without the password a signed, protected document
/// reported no signatures at all. `password` is the one the document was
/// opened with, handed over by its actor for this call; it is not kept.
///
/// For any protected file, `/Contents` is read from the file's bytes rather
/// than from the parse — see [`contents_from_gap`] — and the rest of each
/// signature dictionary from a second parse in which `/Contents` is blanked —
/// see [`with_contents_blanked`].
pub fn verify_signatures_opened_with(
    bytes: &[u8],
    password: Option<&str>,
    now: SystemTime,
) -> Result<Vec<SignatureReport>, CommandError> {
    let parse = |bytes: &[u8]| {
        let read_err = |e| pdf_err("could not read the PDF", e);
        match password {
            Some(password) => load_cos_with_password(bytes, password, read_err),
            None => load_cos(bytes, read_err),
        }
    };
    let doc = parse(bytes)?;
    let protected = declares_encryption(bytes);
    let ids = signature_object_ids(&doc);

    let clean = if protected {
        with_contents_blanked(&doc, bytes, &ids)
            .and_then(|(copy, blanked)| parse(&copy).ok().map(|clean| (clean, blanked)))
    } else {
        None
    };

    let mut reports = Vec::new();
    for id in ids {
        let (source, claims_known) = match &clean {
            Some((clean, blanked)) if blanked.contains(&id) => (clean, true),
            _ => (&doc, !protected),
        };
        let mut report = verify_one(source, bytes, id, now, protected);
        if !claims_known {
            // lopdf may have left them encrypted (see `with_contents_blanked`):
            // unknown is better than garbled.
            report.signed_at = None;
            report.reason = None;
        }
        reports.push(report);
    }
    Ok(reports)
}

/// A protected file's bytes with each signature's `/Contents` replaced by `0`
/// and spaces, every offset kept, and the signatures that was done for.
///
/// lopdf decrypts a dictionary entry by entry and stops at the first entry
/// that fails, and a signature's `/Contents` is the one string ISO 32000 leaves
/// unencrypted. Under AES its decryption fails, so every entry after it stays
/// ciphertext: `/M` and `/Reason` in pyHanko's files, which put `/Contents`
/// first. An integer needs no decryption, so in a parse of this copy the rest
/// of the dictionary decrypts, whatever order its keys are in. Measured
/// 2026-10-21 on `signed-protected.pdf` (RC4 never fails to decrypt, so
/// `signed-protected-rc4.pdf` read correctly already).
///
/// Only a gap that is exactly one hex string is touched, and its length is
/// kept, so the cross-reference offsets stay right and nothing outside the gap
/// changes. What is *verified* never comes from this copy: the digest is over
/// the original bytes, and `/Contents` is read from their gap.
fn with_contents_blanked(
    doc: &Document,
    bytes: &[u8],
    ids: &[ObjectId],
) -> Option<(Vec<u8>, Vec<ObjectId>)> {
    let mut copy: Option<Vec<u8>> = None;
    let mut blanked = Vec::new();
    for id in ids {
        let gap = doc
            .get_object(*id)
            .and_then(Object::as_dict)
            .ok()
            .and_then(|dict| byte_ranges(dict).ok())
            .and_then(|ranges| hex_gap(bytes, &ranges));
        let Some(gap) = gap else { continue };
        blank_gap(copy.get_or_insert_with(|| bytes.to_vec()), gap);
        blanked.push(*id);
    }
    copy.map(|copy| (copy, blanked))
}

/// Overwrite `gap` with spaces and a final `0`. The `0` goes last because the
/// byte before the gap may be the end of the `/Contents` key itself: a `0`
/// there would turn the key into `/Contents0`.
fn blank_gap(bytes: &mut [u8], gap: std::ops::Range<usize>) {
    let Some(last) = gap.end.checked_sub(1).filter(|last| *last >= gap.start) else {
        return;
    };
    bytes[gap.start..last].fill(b' ');
    bytes[last] = b'0';
}

/// Every object that looks like a signature dictionary.
///
/// Found by shape — `/ByteRange` plus `/Contents` — rather than by walking
/// `/AcroForm /Fields`. A signature reachable only through `/Perms /DocMDP`, or
/// one whose field entry a producer wrote oddly, still has to be reported: a
/// signature we fail to notice is one we silently fail to check.
fn signature_object_ids(doc: &Document) -> Vec<ObjectId> {
    let mut ids: Vec<ObjectId> = doc
        .objects
        .iter()
        .filter(|(_, obj)| {
            obj.as_dict()
                .is_ok_and(|d| d.has(b"ByteRange") && d.has(b"Contents"))
        })
        .map(|(id, _)| *id)
        .collect();
    ids.sort_unstable();
    ids
}

/// Build the report for one signature. Never returns `Err`: a signature we
/// cannot parse is a *finding*, not a reason to abandon the whole document.
fn verify_one(
    doc: &Document,
    bytes: &[u8],
    id: ObjectId,
    now: SystemTime,
    protected: bool,
) -> SignatureReport {
    let mut report = SignatureReport {
        field_name: field_name_for(doc, id),
        signer: "unknown".into(),
        issuer: "unknown".into(),
        signed_at: None,
        reason: None,
        signature_valid: false,
        digest_matches: false,
        covers_whole_document: false,
        certificate_expired: false,
        chain: ChainStatus::Incomplete,
        certification_level: None,
        problems: Vec::new(),
    };

    let Ok(dict) = doc.get_object(id).and_then(Object::as_dict) else {
        report.problems.push("The signature entry is unreadable.".into());
        return report;
    };

    report.signed_at = text_of(dict, b"M");
    report.reason = text_of(dict, b"Reason");
    report.certification_level = certification_level(dict);

    let ranges = match byte_ranges(dict) {
        Ok(r) => r,
        Err(msg) => {
            report.problems.push(msg);
            return report;
        }
    };
    report.covers_whole_document = ranges
        .last()
        .is_some_and(|(off, len)| off + len == bytes.len());

    let Some(signed_bytes) = collect_ranges(bytes, &ranges) else {
        report
            .problems
            .push("This file was rewritten or cut short after it was signed, so the signature can no longer be checked.".into());
        return report;
    };

    let blob = if protected { contents_from_gap(bytes, &ranges) } else { contents_der(dict) };
    let Some(blob) = blob else {
        report
            .problems
            .push("The signature has no readable content.".into());
        return report;
    };

    match check_cms(&blob, &signed_bytes, now) {
        Ok(outcome) => {
            report.signer = outcome.signer;
            report.issuer = outcome.issuer;
            report.signature_valid = outcome.signature_valid;
            report.digest_matches = outcome.digest_matches;
            report.certificate_expired = outcome.expired;
            report.chain = outcome.chain;
        }
        Err(msg) => report.problems.push(msg),
    }
    report
}

/// The field whose `/V` points at this signature.
fn field_name_for(doc: &Document, sig: ObjectId) -> Option<String> {
    doc.objects.values().find_map(|obj| {
        let dict = obj.as_dict().ok()?;
        let points_here = dict
            .get(b"V")
            .and_then(Object::as_reference)
            .is_ok_and(|r| r == sig);
        if !points_here {
            return None;
        }
        text_of(dict, b"T")
    })
}

fn text_of(dict: &lopdf::Dictionary, key: &[u8]) -> Option<String> {
    dict.get(key)
        .and_then(Object::as_str)
        .ok()
        .map(|s| String::from_utf8_lossy(s).into_owned())
}

/// `/Reference` → the `DocMDP` `/P`, if this signature certifies.
fn certification_level(dict: &lopdf::Dictionary) -> Option<i64> {
    dict.get(b"Reference")
        .and_then(Object::as_array)
        .ok()?
        .iter()
        .filter_map(|o| o.as_dict().ok())
        .find(|r| {
            r.get(b"TransformMethod")
                .and_then(Object::as_name)
                .is_ok_and(|n| n == b"DocMDP")
        })?
        .get(b"TransformParams")
        .and_then(Object::as_dict)
        .ok()?
        .get(b"P")
        .and_then(Object::as_i64)
        .ok()
}

/// `/ByteRange` as (offset, length) pairs.
fn byte_ranges(dict: &lopdf::Dictionary) -> Result<Vec<(usize, usize)>, String> {
    let array = dict
        .get(b"ByteRange")
        .and_then(Object::as_array)
        .map_err(|_| "The signature does not say which bytes it covers.".to_string())?;
    if array.len() % 2 != 0 || array.is_empty() {
        return Err("The signature's byte range is malformed.".into());
    }

    let mut out = Vec::with_capacity(array.len() / 2);
    for pair in array.chunks(2) {
        let off = pair[0].as_i64().map_err(|_| "byte range".to_string())?;
        let len = pair[1].as_i64().map_err(|_| "byte range".to_string())?;
        let (Ok(off), Ok(len)) = (usize::try_from(off), usize::try_from(len)) else {
            return Err("The signature's byte range is malformed.".into());
        };
        out.push((off, len));
    }
    Ok(out)
}

/// Concatenate the covered ranges, or `None` if any of them runs off the end.
fn collect_ranges(bytes: &[u8], ranges: &[(usize, usize)]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    for (off, len) in ranges {
        let end = off.checked_add(*len)?;
        out.extend_from_slice(bytes.get(*off..end)?);
    }
    Some(out)
}

/// `/Contents` with the placeholder's zero padding removed.
///
/// The gap is a fixed size, so a signature shorter than the gap is followed by
/// zeros. DER decoding demands an exact length, so they have to go; where they
/// start is decided by [`without_padding`].
fn contents_der(dict: &lopdf::Dictionary) -> Option<Vec<u8>> {
    let raw = dict.get(b"Contents").and_then(Object::as_str).ok()?;
    without_padding(raw).map(<[u8]>::to_vec)
}

/// `raw` without the placeholder's zero padding: the one DER value it starts
/// with, its length read from that value's own header.
///
/// **Not "everything up to the last non-zero byte".** That was the rule until
/// 2026-10-18, and a signature's final byte is the RSA signature's own — or a
/// timestamp token's — which is zero about one time in 256. Stripping it
/// handed [`check_cms`] a truncated blob, and a valid signature was reported as
/// unreadable.
///
/// `None` when there is nothing but padding: a reserved field never signed.
/// Anything that is not one DER value followed only by zeros is returned
/// whole, so [`check_cms`] rejects it exactly as it always has.
fn without_padding(raw: &[u8]) -> Option<&[u8]> {
    if raw.iter().all(|b| *b == 0) {
        return None;
    }
    let value_len = der::SliceReader::new(raw)
        .and_then(|mut reader| der::Header::decode(&mut reader))
        .and_then(|header| header.encoded_len()? + header.length)
        .ok()
        .and_then(|len| usize::try_from(len).ok());
    let value = value_len
        .and_then(|len| raw.split_at_checked(len))
        .filter(|(_, padding)| padding.iter().all(|b| *b == 0))
        .map(|(value, _)| value);
    Some(value.unwrap_or(raw))
}

/// `/Contents` as the signer wrote it: the hex string in the gap between the
/// two signed byte ranges, padding removed.
///
/// Used for a protected file. ISO 32000 exempts a signature's `/Contents` from
/// encryption, but lopdf decrypts every string it parses, this one included.
/// With AES that decryption usually fails and leaves the value alone; with RC4
/// it always "succeeds" and returns garbage — measured on 2026-10-17 against a
/// pyHanko-signed RC4 file, whose signature would have read as broken. The
/// gap is where the value sits and exactly what the signed ranges leave out,
/// so reading it there needs no decryption at all.
fn contents_from_gap(bytes: &[u8], ranges: &[(usize, usize)]) -> Option<Vec<u8>> {
    let gap = hex_gap(bytes, ranges)?;
    let hex: Vec<u8> = bytes[gap.start + 1..gap.end - 1]
        .iter()
        .copied()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    if hex.len() % 2 != 0 {
        return None;
    }
    let raw = hex
        .chunks(2)
        .map(|pair| Some(hex_digit(pair[0])? << 4 | hex_digit(pair[1])?))
        .collect::<Option<Vec<u8>>>()?;
    without_padding(&raw).map(<[u8]>::to_vec)
}

/// Where `/Contents` sits: the gap between the two signed ranges, when it is
/// exactly one hex string, delimiters included.
fn hex_gap(bytes: &[u8], ranges: &[(usize, usize)]) -> Option<std::ops::Range<usize>> {
    let [(first, len), (second, _)] = ranges else {
        return None;
    };
    let start = first.checked_add(*len)?;
    let inner = bytes.get(start..*second)?.strip_prefix(b"<")?.strip_suffix(b">")?;
    inner
        .iter()
        .all(|b| b.is_ascii_hexdigit() || b.is_ascii_whitespace())
        .then_some(start..*second)
}

fn hex_digit(c: u8) -> Option<u8> {
    char::from(c).to_digit(16).and_then(|d| u8::try_from(d).ok())
}

struct CmsOutcome {
    signer: String,
    issuer: String,
    signature_valid: bool,
    digest_matches: bool,
    expired: bool,
    chain: ChainStatus,
}

/// The cryptographic half: digest, signature, certificate.
fn check_cms(blob: &[u8], signed_bytes: &[u8], now: SystemTime) -> Result<CmsOutcome, String> {
    let info = ContentInfo::from_der(blob)
        .map_err(|_| "The signature is not a readable PKCS#7 structure.".to_string())?;
    if info.content_type != ID_SIGNED_DATA {
        return Err("The signature is not of a kind VibePDF can check.".into());
    }
    let signed: SignedData = info
        .content
        .decode_as()
        .map_err(|_| "The signature's contents are malformed.".to_string())?;

    let signer_info = signed
        .signer_infos
        .0
        .as_ref()
        .first()
        .ok_or_else(|| "The signature names no signer.".to_string())?;

    let cert = signer_certificate(&signed, signer_info)
        .ok_or_else(|| "The signing certificate is not in the file.".to_string())?;

    let digest_matches = message_digest_matches(signer_info, signed_bytes)?;
    let signature_valid = signature_checks_out(signer_info, &cert);
    let expired = is_expired(&cert, now);
    let chain = chain_status(&signed, &cert);

    Ok(CmsOutcome {
        signer: cert.tbs_certificate.subject.to_string(),
        issuer: cert.tbs_certificate.issuer.to_string(),
        signature_valid,
        digest_matches,
        expired,
        chain,
    })
}

/// The embedded certificate whose issuer and serial match the signer identifier.
fn signer_certificate(signed: &SignedData, info: &SignerInfo) -> Option<Certificate> {
    use cms::cert::CertificateChoices;
    use cms::signed_data::SignerIdentifier;

    let SignerIdentifier::IssuerAndSerialNumber(wanted) = &info.sid else {
        // SubjectKeyIdentifier is legal and we do not match on it yet; say so
        // rather than picking the first certificate and hoping.
        return None;
    };
    signed.certificates.as_ref()?.0.iter().find_map(|choice| {
        let CertificateChoices::Certificate(cert) = choice else {
            return None;
        };
        let tbs = &cert.tbs_certificate;
        (tbs.issuer == wanted.issuer && tbs.serial_number == wanted.serial_number)
            .then(|| cert.clone())
    })
}

/// Does the document still hash to what was signed?
fn message_digest_matches(info: &SignerInfo, signed_bytes: &[u8]) -> Result<bool, String> {
    if info.digest_alg.oid != ID_SHA_256 {
        return Err("The signature uses a digest VibePDF cannot check.".into());
    }
    let attrs = info
        .signed_attrs
        .as_ref()
        .ok_or_else(|| "The signature has no signed attributes.".to_string())?;

    let claimed = attrs
        .iter()
        .find(|a| a.oid == ID_MESSAGE_DIGEST)
        .and_then(|a| a.values.as_ref().first())
        .and_then(|v| v.decode_as::<der::asn1::OctetString>().ok())
        .ok_or_else(|| "The signature does not say what it signed.".to_string())?;

    Ok(claimed.as_bytes() == Sha256::digest(signed_bytes).as_slice())
}

/// Does the RSA signature over the signed attributes check out?
///
/// The signed bytes are the attributes re-encoded as a DER `SET OF` — not the
/// `[0] IMPLICIT` form they appear in inside the structure. Signing one and
/// verifying the other is the classic way to get a verifier that rejects every
/// valid signature.
fn signature_checks_out(info: &SignerInfo, cert: &Certificate) -> bool {
    use rsa::pkcs1v15::{Signature, VerifyingKey};
    use rsa::pkcs8::DecodePublicKey;
    use rsa::signature::Verifier;
    use rsa::RsaPublicKey;

    let Some(attrs) = info.signed_attrs.as_ref() else {
        return false;
    };
    let Ok(message) = attrs.to_der() else {
        return false;
    };
    let Ok(spki) = cert.tbs_certificate.subject_public_key_info.to_der() else {
        return false;
    };
    let Ok(public) = RsaPublicKey::from_public_key_der(&spki) else {
        return false;
    };
    let Ok(signature) = Signature::try_from(info.signature.as_bytes()) else {
        return false;
    };
    VerifyingKey::<Sha256>::new(public)
        .verify(&message, &signature)
        .is_ok()
}

/// Was the certificate outside its validity window at `now`?
fn is_expired(cert: &Certificate, now: SystemTime) -> bool {
    let validity = &cert.tbs_certificate.validity;
    now > validity.not_after.to_system_time() || now < validity.not_before.to_system_time()
}

/// What shape is the chain — never whether it is trusted.
///
/// The certificates are *checked* against each other even though they cannot be
/// checked against a trust list. That distinction is easy to skip and expensive
/// to skip: the signature commits to the signed attributes, not to the
/// certificate carried alongside them, so altering a byte of the embedded
/// certificate leaves the signature verifying perfectly while the name shown
/// next to it says whatever the attacker chose. Following each link puts a stop
/// to that.
fn chain_status(blob: &SignedData, signer: &Certificate) -> ChainStatus {
    use cms::cert::CertificateChoices;

    if signer.tbs_certificate.subject == signer.tbs_certificate.issuer {
        return if certificate_signed_by(signer, signer) {
            ChainStatus::SelfSigned
        } else {
            ChainStatus::Broken
        };
    }

    let issuer = blob.certificates.as_ref().and_then(|set| {
        set.0.iter().find_map(|choice| {
            let CertificateChoices::Certificate(c) = choice else {
                return None;
            };
            (c.tbs_certificate.subject == signer.tbs_certificate.issuer).then_some(c)
        })
    });

    match issuer {
        None => ChainStatus::Incomplete,
        Some(issuer) if certificate_signed_by(signer, issuer) => ChainStatus::IssuerNotChecked,
        Some(_) => ChainStatus::Broken,
    }
}

/// Does `subject`'s own signature verify against `issuer`'s public key?
///
/// Only the RSA-with-SHA-2 family. An algorithm we do not implement returns
/// `true` — "not checked" rather than "failed", because reporting a chain as
/// broken when we simply cannot read it would be its own kind of lie. The
/// `ChainStatus` already says trust is unverified either way.
fn certificate_signed_by(subject: &Certificate, issuer: &Certificate) -> bool {
    use const_oid::db::rfc5912::{
        SHA_256_WITH_RSA_ENCRYPTION, SHA_384_WITH_RSA_ENCRYPTION, SHA_512_WITH_RSA_ENCRYPTION,
    };
    use rsa::pkcs1v15::{Signature, VerifyingKey};
    use rsa::pkcs8::DecodePublicKey;
    use rsa::signature::Verifier;
    use rsa::RsaPublicKey;

    let (Ok(tbs), Some(sig_bytes)) = (
        subject.tbs_certificate.to_der(),
        subject.signature.as_bytes(),
    ) else {
        return false;
    };
    let Ok(spki) = issuer.tbs_certificate.subject_public_key_info.to_der() else {
        return false;
    };
    let Ok(public) = RsaPublicKey::from_public_key_der(&spki) else {
        return true; // not an RSA issuer: unchecked, not failed
    };
    let Ok(signature) = Signature::try_from(sig_bytes) else {
        return false;
    };

    match subject.signature_algorithm.oid {
        SHA_256_WITH_RSA_ENCRYPTION => VerifyingKey::<Sha256>::new(public)
            .verify(&tbs, &signature)
            .is_ok(),
        SHA_384_WITH_RSA_ENCRYPTION => VerifyingKey::<sha2::Sha384>::new(public)
            .verify(&tbs, &signature)
            .is_ok(),
        SHA_512_WITH_RSA_ENCRYPTION => VerifyingKey::<sha2::Sha512>::new(public)
            .verify(&tbs, &signature)
            .is_ok(),
        _ => true, // unchecked, not failed
    }
}

#[cfg(test)]
mod tests {
    use super::{blank_gap, contents_der, contents_from_gap, hex_gap, without_padding};

    /// SEQUENCE { INTEGER 0 }: a complete DER value whose last byte is 0x00.
    const ENDS_IN_ZERO: [u8; 5] = [0x30, 0x03, 0x02, 0x01, 0x00];

    /// `prefix`, then `gap`, then `suffix`, with the two ranges around the gap.
    fn file(gap: &[u8]) -> (Vec<u8>, Vec<(usize, usize)>) {
        let (prefix, suffix) = (b"%PDF-1.7 /Contents ".as_slice(), b" /ByteRange [...] %%EOF".as_slice());
        let bytes = [prefix, gap, suffix].concat();
        let second = prefix.len() + gap.len();
        (bytes, vec![(0, prefix.len()), (second, suffix.len())])
    }

    // Only a gap that is exactly one hex string is `/Contents`: anything else
    // is left alone rather than blanked on a guess.
    #[test]
    fn the_gap_is_found_only_when_it_is_one_hex_string() {
        let (bytes, ranges) = file(b"<3003 0201\n00 0000>");
        let prefix = ranges[0].1;
        assert_eq!(hex_gap(&bytes, &ranges), Some(prefix..ranges[1].0));

        for not_hex in [&b"<30zz>"[..], b"(3003)", b"<3003", b"3003>"] {
            let (bytes, ranges) = file(not_hex);
            assert_eq!(hex_gap(&bytes, &ranges), None, "{}", String::from_utf8_lossy(not_hex));
        }
        // One byte off either way, as a shifted /ByteRange would put it.
        let (bytes, mut ranges) = file(b"<3003>");
        ranges[0].1 += 1;
        assert_eq!(hex_gap(&bytes, &ranges), None);
        // Not two ranges around one gap.
        assert_eq!(hex_gap(&bytes, &[(0, 4)]), None);
    }

    // Spaces, then the `0`: the key before the gap may end right at it.
    #[test]
    fn blanking_rewrites_the_gap_and_nothing_else() {
        let mut bytes = b"<</Contents<a1b2>/M(x)>>".to_vec();
        let original = bytes.clone();
        blank_gap(&mut bytes, 11..17);
        assert_eq!(bytes, b"<</Contents     0/M(x)>>".to_vec());
        assert_eq!(bytes.len(), original.len());
        assert_eq!((&bytes[..11], &bytes[17..]), (&original[..11], &original[17..]));

        let mut empty = b"/Contents<>>>".to_vec();
        blank_gap(&mut empty, 9..11);
        assert_eq!(empty, b"/Contents 0>>".to_vec());
    }

    #[test]
    fn the_gap_decodes_to_its_bytes_without_the_padding() {
        let (bytes, ranges) = file(b"<3003 0201\n00 0000>");
        assert_eq!(contents_from_gap(&bytes, &ranges), Some(ENDS_IN_ZERO.to_vec()));
    }

    // The bug `without_padding` replaced: the padding was found by stripping
    // trailing zeros, which also stripped a value's own final 0x00 — the RSA
    // signature's last byte, so about one valid signature in 256.
    #[test]
    fn a_value_ending_in_zero_keeps_its_last_byte() {
        let padded = [ENDS_IN_ZERO.as_slice(), &[0; 8]].concat();
        assert_eq!(without_padding(&padded), Some(ENDS_IN_ZERO.as_slice()));

        // Long-form length, as a real signature's is: SEQUENCE (131 bytes)
        // { OCTET STRING (128 bytes), the last of them 0x00 }.
        let mut long = vec![0x30, 0x81, 0x83, 0x04, 0x81, 0x80];
        long.extend([0xAB; 127]);
        long.push(0x00);
        let padded = [long.as_slice(), &[0; 16]].concat();
        assert_eq!(without_padding(&padded), Some(long.as_slice()));
    }

    #[test]
    fn both_readers_keep_a_final_zero() {
        let mut dict = lopdf::Dictionary::new();
        let padded = [ENDS_IN_ZERO.as_slice(), &[0; 8]].concat();
        dict.set("Contents", lopdf::Object::String(padded, lopdf::StringFormat::Hexadecimal));
        assert_eq!(contents_der(&dict), Some(ENDS_IN_ZERO.to_vec()));

        let (bytes, ranges) = file(b"<300302010000000000>");
        assert_eq!(contents_from_gap(&bytes, &ranges), Some(ENDS_IN_ZERO.to_vec()));
    }

    #[test]
    fn a_value_with_no_padding_comes_back_as_it_is() {
        assert_eq!(without_padding(&ENDS_IN_ZERO), Some(ENDS_IN_ZERO.as_slice()));
    }

    // A reserved field that was never signed: "no readable content", not a
    // malformed signature.
    #[test]
    fn nothing_but_padding_is_no_content() {
        assert_eq!(without_padding(&[0; 32]), None);
        assert_eq!(without_padding(&[]), None);
    }

    // Each of these was unreadable before, and is still handed on whole so it
    // stays unreadable — never trimmed into something that might parse.
    #[test]
    fn anything_but_one_value_then_zeros_is_returned_whole() {
        let junk_after = [ENDS_IN_ZERO.as_slice(), &[0x00, 0x01, 0x00]].concat();
        // A header claiming 511 bytes, with two present.
        let too_long = [0x30, 0x82, 0x01, 0xff, 0x00, 0x00];
        // Indefinite length: BER, which DER forbids.
        let indefinite = [0x30, 0x80, 0x02, 0x01, 0x00, 0x00, 0x00];
        for raw in [junk_after.as_slice(), &too_long, &indefinite] {
            assert_eq!(without_padding(raw), Some(raw), "{raw:02x?}");
        }
    }

    #[test]
    fn anything_but_one_hex_string_in_the_gap_is_unreadable() {
        for gap in [b"3082".as_slice(), b"<308>", b"<30zz>", b"(0)", b"<3082"] {
            let (bytes, ranges) = file(gap);
            assert_eq!(contents_from_gap(&bytes, &ranges), None, "{:?}", String::from_utf8_lossy(gap));
        }
    }

    // One gap is the shape the standard describes; anything else is not a
    // place a signature can be read from.
    #[test]
    fn ranges_that_are_not_two_have_no_gap() {
        let (bytes, mut ranges) = file(b"<30>");
        ranges.push((bytes.len(), 0));
        assert_eq!(contents_from_gap(&bytes, &ranges), None);
        assert_eq!(contents_from_gap(&bytes, &ranges[..1]), None);
    }

    #[test]
    fn ranges_past_the_end_are_unreadable_not_a_panic() {
        let (bytes, _) = file(b"<30>");
        assert_eq!(contents_from_gap(&bytes, &[(0, 10), (bytes.len() + 5, 1)]), None);
        assert_eq!(contents_from_gap(&bytes, &[(usize::MAX, 2), (3, 1)]), None);
    }
}
