//! The CMS `SignedData` blob that goes in `/Contents` (P6.B1a, SPEC P6-SEC-005).
//!
//! **No cryptography is implemented here.** `cms::builder` assembles the
//! structure and computes the signature; `rsa` owns RSASSA-PKCS1-v1_5; `sha2`
//! owns the digest. This file chooses the parameters and adds the one attribute
//! the builder does not know about.
//!
//! ## Detached, and what that means
//!
//! A PDF signature is a *detached* CMS: `eContent` is absent, and what the
//! signature actually commits to is the `message-digest` signed attribute — a
//! SHA-256 over the bytes `/ByteRange` names. So the chain of custody is
//!
//! ```text
//! document bytes → SHA-256 → message-digest attribute
//!                            → SignedAttributes (DER, SET OF)
//!                            → RSA signature → /Contents
//! ```
//!
//! and a verifier walks it backwards. Every link is somewhere a wrong byte
//! produces "invalid signature" with no further explanation, which is why the
//! digest is computed in exactly one place and passed through.
//!
//! ## `PAdES` conformance
//!
//! `/SubFilter /ETSI.CAdES.detached` is a claim about which rules the blob
//! follows, and EN 319 142 requires the `signing-certificate-v2` attribute:
//! a hash of the signing certificate, signed alongside everything else, so an
//! attacker cannot swap in a different certificate with the same key. The
//! builder does not add it, so this module does — claiming a profile we do not
//! meet would be worse than not claiming it.

use cms::builder::{create_signing_time_attribute, SignedDataBuilder, SignerInfoBuilder};
use cms::cert::{CertificateChoices, IssuerAndSerialNumber};
use cms::content_info::ContentInfo;
use cms::signed_data::{EncapsulatedContentInfo, SignerIdentifier};
use const_oid::db::rfc5911::ID_DATA;
use const_oid::db::rfc5912::ID_SHA_256;
use const_oid::ObjectIdentifier;
use der::asn1::{OctetString, SetOfVec};
use der::{Any, Encode, Tag};
use rsa::pkcs1v15::SigningKey;
use sha2::{Digest, Sha256};
use x509_cert::attr::Attribute;
use x509_cert::spki::AlgorithmIdentifierOwned;
use x509_cert::Certificate;

use crate::error::CommandError;
use crate::security::credential::SigningCredential;

/// ESS `signing-certificate-v2`, RFC 5035 §3.
const SIGNING_CERTIFICATE_V2: ObjectIdentifier =
    ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.16.2.47");

fn cms_err(what: &str, e: impl std::fmt::Display) -> CommandError {
    CommandError::Internal(format!("cms: {what}: {e}"))
}

/// SPEC: P6-SEC-005 — a detached CMS `SignedData` over `message`.
///
/// `message` is the document with the signature gap cut out, exactly as
/// `/ByteRange` describes it. The certificate chain from the `.pfx` travels
/// inside the blob, which is what lets a verifier build a path without having
/// the intermediates itself.
pub fn sign_detached(
    message: &[u8],
    credential: &SigningCredential,
) -> Result<Vec<u8>, CommandError> {
    let digest = Sha256::digest(message);

    // Detached: no eContent. The signature commits to the digest above, carried
    // as a signed attribute.
    let encap = EncapsulatedContentInfo {
        econtent_type: ID_DATA,
        econtent: None,
    };

    let sha256 = AlgorithmIdentifierOwned {
        oid: ID_SHA_256,
        parameters: None,
    };

    // Issuer + serial is how a verifier finds which certificate signed, and it
    // has to match the embedded one exactly.
    let sid = SignerIdentifier::IssuerAndSerialNumber(IssuerAndSerialNumber {
        issuer: credential.signer.tbs_certificate.issuer.clone(),
        serial_number: credential.signer.tbs_certificate.serial_number.clone(),
    });

    let signing_key = SigningKey::<Sha256>::new(credential.key.clone());
    let mut signer_info = SignerInfoBuilder::new(
        &signing_key,
        sid,
        sha256.clone(),
        &encap,
        Some(digest.as_slice()),
    )
    .map_err(|e| cms_err("preparing the signer", e))?;

    signer_info
        .add_signed_attribute(signing_certificate_v2(&credential.signer)?)
        .map_err(|e| cms_err("adding signing-certificate-v2", e))?;
    signer_info
        .add_signed_attribute(
            create_signing_time_attribute().map_err(|e| cms_err("signing time", e))?,
        )
        .map_err(|e| cms_err("adding signing time", e))?;

    let mut builder = SignedDataBuilder::new(&encap);
    builder
        .add_digest_algorithm(sha256)
        .map_err(|e| cms_err("adding the digest algorithm", e))?;

    // The signer first, then the chain: a verifier is free to ignore the order,
    // but every tool that prints a chain prints it in the order it was given.
    for cert in std::iter::once(&credential.signer).chain(credential.chain.iter()) {
        builder
            .add_certificate(CertificateChoices::Certificate(cert.clone()))
            .map_err(|e| cms_err("embedding a certificate", e))?;
    }

    builder
        .add_signer_info::<SigningKey<Sha256>, rsa::pkcs1v15::Signature>(signer_info)
        .map_err(|e| cms_err("signing", e))?;

    let signed: ContentInfo = builder.build().map_err(|e| cms_err("assembling", e))?;
    signed.to_der().map_err(|e| cms_err("encoding", e))
}

/// RFC 5035 `SigningCertificateV2`, in its minimal form.
///
/// ```text
/// SigningCertificateV2 ::= SEQUENCE {
///     certs SEQUENCE OF ESSCertIDv2 }
/// ESSCertIDv2 ::= SEQUENCE {
///     hashAlgorithm AlgorithmIdentifier DEFAULT id-sha256,
///     certHash      OCTET STRING }
/// ```
///
/// `hashAlgorithm` is omitted because SHA-256 is its DEFAULT, and DER requires
/// a field equal to its default to be *absent* — writing it out would be a
/// non-canonical encoding that strict verifiers reject.
fn signing_certificate_v2(signer: &Certificate) -> Result<Attribute, CommandError> {
    let der = signer
        .to_der()
        .map_err(|e| cms_err("re-encoding the signing certificate", e))?;
    let hash = OctetString::new(Sha256::digest(&der).to_vec())
        .map_err(|e| cms_err("wrapping the certificate hash", e))?;

    let ess_cert_id = sequence(&hash.to_der().map_err(|e| cms_err("cert hash", e))?)?;
    let certs = sequence(&ess_cert_id.to_der().map_err(|e| cms_err("ESSCertIDv2", e))?)?;
    let value = sequence(&certs.to_der().map_err(|e| cms_err("cert list", e))?)?;

    let mut values = SetOfVec::new();
    values
        .insert(value)
        .map_err(|e| cms_err("building the attribute", e))?;

    Ok(Attribute {
        oid: SIGNING_CERTIFICATE_V2,
        values,
    })
}

/// Wrap already-encoded DER in a SEQUENCE.
fn sequence(contents: &[u8]) -> Result<Any, CommandError> {
    Any::new(Tag::Sequence, contents).map_err(|e| cms_err("building a SEQUENCE", e))
}
