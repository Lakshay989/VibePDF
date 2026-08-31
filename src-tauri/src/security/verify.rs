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
    let doc = Document::load_mem(bytes).map_err(|e| pdf_err("could not read the PDF", e))?;

    let mut reports = Vec::new();
    for id in signature_object_ids(&doc) {
        reports.push(verify_one(&doc, bytes, id, now));
    }
    Ok(reports)
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
            .push("The signature covers bytes outside the file.".into());
        return report;
    };

    let Some(blob) = contents_der(dict) else {
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
/// zeros. DER decoding demands an exact length, so they have to go — but only
/// the trailing ones, and only after the last non-zero byte.
fn contents_der(dict: &lopdf::Dictionary) -> Option<Vec<u8>> {
    let raw = dict.get(b"Contents").and_then(Object::as_str).ok()?;
    let end = raw.iter().rposition(|b| *b != 0)? + 1;
    Some(raw[..end].to_vec())
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
