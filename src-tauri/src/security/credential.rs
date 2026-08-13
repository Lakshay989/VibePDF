//! Unwrapping a PKCS#12 (`.pfx`) file into a key and a certificate chain
//! (P6.B1a, SPEC P6-SEC-005).
//!
//! **No cryptography is implemented here.** The KDF is `pkcs12::kdf` (RFC 7292
//! Appendix B), the decryption is `pkcs5`, the key parsing is `pkcs8`, and the
//! certificates are `x509-cert`. What this file does is walk a nested container
//! format and hand the pieces to the crates that own them.
//!
//! ## Why this is three crates and not one
//!
//! `p12` 0.6.3 does it in one call, and was rejected: it hard-asserts a SHA-1
//! MAC and *panics* on any `.pfx` OpenSSL 3 produces by default, which uses
//! SHA-256. Verified against both, recorded in `steps/P6.md`.
//!
//! ## What a `.pfx` actually is
//!
//! Four layers, each of which can be encrypted separately:
//!
//! ```text
//! PFX
//!  └── authSafe: ContentInfo (data)          ← MAC covers this, keyed by password
//!       └── AuthenticatedSafe: [ContentInfo] ← one per "safe"
//!            ├── data          (plaintext)   ← usually the shrouded key
//!            └── encryptedData (PBES2/PBE)   ← usually the certificates
//!                 └── SafeBag                ← key, shrouded key, or cert
//! ```
//!
//! The password is used three different ways — MAC key, safe decryption, key
//! decryption — with different KDF `id` values. Getting one right and another
//! wrong produces "wrong password" on a correct password, so the failures here
//! are named individually rather than collapsed.

use cms::content_info::ContentInfo;
use der::asn1::OctetString;
use der::{Decode, Encode};
use pkcs12::authenticated_safe::AuthenticatedSafe;
use pkcs12::cert_type::CertBag;
use pkcs12::pfx::Pfx;
use pkcs12::safe_bag::SafeContents;
use rsa::RsaPrivateKey;
use x509_cert::Certificate;

use crate::error::CommandError;

/// OIDs we have to recognise by hand, because they identify *containers* rather
/// than algorithms a crate would dispatch on.
mod oid {
    use const_oid::ObjectIdentifier;

    /// `pkcs-7 data`
    pub const DATA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.1");
    /// `pkcs-7 encryptedData`
    pub const ENCRYPTED_DATA: ObjectIdentifier =
        ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.6");
    /// `pkcs-12 keyBag`
    pub const KEY_BAG: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.12.10.1.1");
    /// `pkcs-12 pkcs8ShroudedKeyBag`
    pub const SHROUDED_KEY_BAG: ObjectIdentifier =
        ObjectIdentifier::new_unwrap("1.2.840.113549.1.12.10.1.2");
    /// `pkcs-12 certBag`
    pub const CERT_BAG: ObjectIdentifier =
        ObjectIdentifier::new_unwrap("1.2.840.113549.1.12.10.1.3");
}

/// A private key and the certificates that go with it.
///
/// `signer` is the certificate matching the key — the one whose issuer and
/// serial identify the signature. `chain` is everything else in the file:
/// intermediates a verifier needs and will not otherwise have. Both are
/// embedded in the CMS blob.
pub struct SigningCredential {
    pub key: RsaPrivateKey,
    pub signer: Certificate,
    pub chain: Vec<Certificate>,
}

impl std::fmt::Debug for SigningCredential {
    /// Never prints the key. A `{:?}` in a log or a test failure is exactly how
    /// private key material escapes.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SigningCredential")
            .field("signer", &self.signer.tbs_certificate.subject.to_string())
            .field("chain_len", &self.chain.len())
            .finish_non_exhaustive()
    }
}

fn bad_input(msg: impl Into<String>) -> CommandError {
    CommandError::InvalidInput(msg.into())
}

/// SPEC: P6-SEC-005 — read a PKCS#12 file into a signing credential.
///
/// `password` is never echoed into an error. "Wrong password" and "unsupported
/// format" are told apart where possible, because a user retyping a correct
/// password is the worst outcome here.
pub fn load_pkcs12(der: &[u8], password: &str) -> Result<SigningCredential, CommandError> {
    let pfx = Pfx::from_der(der)
        .map_err(|_| bad_input("That file isn't a PKCS#12 (.pfx / .p12) certificate."))?;

    if pfx.auth_safe.content_type != oid::DATA {
        return Err(bad_input(
            "This .pfx uses a password-protected outer layer VibePDF can't read.",
        ));
    }
    let auth_safe_der = pfx
        .auth_safe
        .content
        .to_der()
        .map_err(|e| CommandError::Internal(format!("pkcs12: re-encoding authSafe: {e}")))?;
    let inner = OctetString::from_der(&auth_safe_der)
        .map_err(|_| bad_input("This .pfx is malformed: its authenticated safe isn't a string."))?;

    verify_mac(&pfx, inner.as_bytes(), password)?;

    let safes = AuthenticatedSafe::from_der(inner.as_bytes())
        .map_err(|_| bad_input("This .pfx is malformed: its safe contents wouldn't parse."))?;

    let mut keys: Vec<RsaPrivateKey> = Vec::new();
    let mut certs: Vec<Certificate> = Vec::new();
    for safe in &safes {
        let contents = decrypt_safe(safe, password)?;
        collect_bags(&contents, password, &mut keys, &mut certs)?;
    }

    let key = keys
        .into_iter()
        .next()
        .ok_or_else(|| bad_input("That .pfx has no private key in it, so it can't sign."))?;
    let signer = pick_signer(&key, &mut certs)?;

    Ok(SigningCredential {
        key,
        signer,
        chain: certs,
    })
}

/// Check the MAC, which is what actually tells us the password is right.
///
/// The digest is whatever the file says — SHA-1 on older files, SHA-256 on
/// anything OpenSSL 3 wrote. Assuming SHA-1 here is precisely the bug that
/// ruled out `p12`.
fn verify_mac(pfx: &Pfx, data: &[u8], password: &str) -> Result<(), CommandError> {
    use const_oid::db::rfc5912::{ID_SHA_1, ID_SHA_256, ID_SHA_384, ID_SHA_512};

    let Some(mac) = pfx.mac_data.as_ref() else {
        // No MAC is legal and means no integrity check; a wrong password then
        // surfaces later as a decryption failure instead.
        return Ok(());
    };

    let salt = mac.mac_salt.as_bytes();
    let rounds = mac.iterations;
    let expected = mac.mac.digest.as_bytes();

    let computed = match mac.mac.algorithm.oid {
        ID_SHA_1 => compute_mac::<sha1::Sha1>(password, salt, rounds, data)?,
        ID_SHA_256 => compute_mac::<sha2::Sha256>(password, salt, rounds, data)?,
        ID_SHA_384 => compute_mac::<sha2::Sha384>(password, salt, rounds, data)?,
        ID_SHA_512 => compute_mac::<sha2::Sha512>(password, salt, rounds, data)?,
        other => {
            return Err(bad_input(format!(
                "This .pfx protects itself with a digest VibePDF doesn't support ({other})."
            )))
        }
    };

    if computed == expected {
        Ok(())
    } else {
        Err(bad_input("That password doesn't open this certificate file."))
    }
}

/// RFC 7292 §4 — HMAC over the authenticated safe, keyed by the PKCS#12 KDF.
fn compute_mac<D>(
    password: &str,
    salt: &[u8],
    rounds: i32,
    data: &[u8],
) -> Result<Vec<u8>, CommandError>
where
    D: digest::Digest + digest::FixedOutputReset + digest::core_api::BlockSizeUser + Clone,
{
    use hmac::{Mac, SimpleHmac};
    use pkcs12::kdf::{derive_key_utf8, Pkcs12KeyType};

    let len = <D as digest::Digest>::output_size();
    let key = derive_key_utf8::<D>(password, salt, Pkcs12KeyType::Mac, rounds, len)
        .map_err(|e| CommandError::Internal(format!("pkcs12: MAC key derivation: {e}")))?;
    let mut hmac = <SimpleHmac<D> as Mac>::new_from_slice(&key)
        .map_err(|_| CommandError::Internal("pkcs12: HMAC rejected the derived key".into()))?;
    hmac.update(data);
    Ok(hmac.finalize().into_bytes().to_vec())
}

/// One safe: either plaintext `data` or `encryptedData` we have to unwrap.
fn decrypt_safe(safe: &ContentInfo, password: &str) -> Result<SafeContents, CommandError> {
    let der = safe
        .content
        .to_der()
        .map_err(|e| CommandError::Internal(format!("pkcs12: re-encoding a safe: {e}")))?;

    let plaintext = if safe.content_type == oid::DATA {
        OctetString::from_der(&der)
            .map_err(|_| bad_input("This .pfx has a malformed plaintext safe."))?
            .as_bytes()
            .to_vec()
    } else if safe.content_type == oid::ENCRYPTED_DATA {
        decrypt_encrypted_data(&der, password)?
    } else {
        // Nothing else is defined for a PKCS#12 safe; skipping is safer than
        // guessing, and a file made only of unknown safes fails later with
        // "no private key" rather than silently signing with nothing.
        return Ok(SafeContents::default());
    };

    SafeContents::from_der(&plaintext)
        .map_err(|_| bad_input("This .pfx decrypted to something that isn't a safe."))
}

/// `encryptedData` → plaintext, via whatever scheme the file names.
fn decrypt_encrypted_data(der: &[u8], password: &str) -> Result<Vec<u8>, CommandError> {
    use cms::encrypted_data::EncryptedData;

    let enc = EncryptedData::from_der(der)
        .map_err(|_| bad_input("This .pfx has a malformed encrypted safe."))?;
    let params = enc
        .enc_content_info
        .content_enc_alg
        .to_der()
        .map_err(|e| CommandError::Internal(format!("pkcs12: re-encoding a scheme: {e}")))?;
    let ciphertext = enc
        .enc_content_info
        .encrypted_content
        .ok_or_else(|| bad_input("This .pfx has an encrypted safe with nothing in it."))?;

    decrypt_with_scheme(&params, ciphertext.as_bytes(), password)
}

/// PBES2 (modern) or a PKCS#12 PBE (legacy), told apart by OID.
fn decrypt_with_scheme(
    alg_der: &[u8],
    ciphertext: &[u8],
    password: &str,
) -> Result<Vec<u8>, CommandError> {
    use pkcs5::EncryptionScheme;

    // `pkcs5` covers PBES1 and PBES2, which is every modern .pfx.
    if let Ok(scheme) = EncryptionScheme::from_der(alg_der) {
        return scheme.decrypt(password, ciphertext).map_err(|_| {
            bad_input("That password doesn't open this certificate file.")
        });
    }
    legacy_pkcs12_pbe(alg_der, ciphertext, password)
}

/// The pre-PBES2 family: `pbeWithSHAAnd3-KeyTripleDES-CBC` and friends.
///
/// These are not PKCS#5 schemes, so `pkcs5` does not recognise them. The KDF is
/// RFC 7292's (`pkcs12::kdf`, already in hand for the MAC) and the cipher is
/// 3DES-CBC; both come from crates, and this is the wiring between them.
///
/// Only the 3DES variants are handled. The 40-bit RC2 ones in the same family
/// are refused by name rather than implemented: they are broken, and a file
/// still using them should be re-exported rather than read.
fn legacy_pkcs12_pbe(
    alg_der: &[u8],
    ciphertext: &[u8],
    password: &str,
) -> Result<Vec<u8>, CommandError> {
    use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
    use const_oid::ObjectIdentifier;
    use pkcs12::kdf::{derive_key_utf8, Pkcs12KeyType};
    use x509_cert::spki::AlgorithmIdentifierOwned;

    /// `pbeWithSHAAnd3-KeyTripleDES-CBC`
    const SHA1_3DES: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.12.1.3");
    /// `pbeWithSHAAnd2-KeyTripleDES-CBC`
    const SHA1_2KEY_3DES: ObjectIdentifier =
        ObjectIdentifier::new_unwrap("1.2.840.113549.1.12.1.4");

    let alg = AlgorithmIdentifierOwned::from_der(alg_der)
        .map_err(|_| bad_input("This .pfx names an encryption scheme VibePDF can't parse."))?;

    let key_len = match alg.oid {
        SHA1_3DES => 24,
        SHA1_2KEY_3DES => 16,
        other => {
            return Err(bad_input(format!(
                "This .pfx uses an obsolete encryption scheme VibePDF doesn't support ({other}). \
                 Re-export it from your certificate manager and try again."
            )))
        }
    };

    let params = alg
        .parameters
        .ok_or_else(|| bad_input("This .pfx is missing its encryption parameters."))?
        .to_der()
        .map_err(|e| CommandError::Internal(format!("pkcs12: re-encoding params: {e}")))?;
    let pbe = pkcs12::pbe_params::Pkcs12PbeParams::from_der(&params)
        .map_err(|_| bad_input("This .pfx has malformed encryption parameters."))?;

    let salt = pbe.salt.as_bytes();
    let rounds = pbe.iterations;
    let key = derive_key_utf8::<sha1::Sha1>(
        password,
        salt,
        Pkcs12KeyType::EncryptionKey,
        rounds,
        key_len,
    )
    .map_err(|e| CommandError::Internal(format!("pkcs12: KDF: {e}")))?;
    let iv = derive_key_utf8::<sha1::Sha1>(password, salt, Pkcs12KeyType::Iv, rounds, 8)
        .map_err(|e| CommandError::Internal(format!("pkcs12: KDF: {e}")))?;

    // A 2-key 3DES key is K1||K2; the cipher wants K1||K2||K1.
    let mut full = key.clone();
    if key_len == 16 {
        full.extend_from_slice(&key[..8]);
    }

    let dec = <cbc::Decryptor<des::TdesEde3>>::new_from_slices(&full, &iv)
        .map_err(|_| CommandError::Internal("pkcs12: bad 3DES key length".into()))?;
    dec.decrypt_padded_vec_mut::<Pkcs7>(ciphertext)
        .map_err(|_| bad_input("That password doesn't open this certificate file."))
}

/// `EncryptedPrivateKeyInfo` → the PKCS#8 key inside it.
///
/// Deliberately not `pkcs8::EncryptedPrivateKeyInfo::decrypt`: that type parses
/// its algorithm field as a `pkcs5` scheme, so a legacy `.pfx` — whose key is
/// shrouded with a PKCS#12 PBE rather than a PKCS#5 one — fails to *parse*, and
/// reports itself as a malformed key rather than as an old one. Reading the two
/// fields directly lets both flavours go through the same dispatch.
fn decrypt_shrouded_key(der: &[u8], password: &str) -> Result<Vec<u8>, CommandError> {
    use der::{Decode, SliceReader};
    use x509_cert::spki::AlgorithmIdentifierOwned;

    let seq = der::Any::from_der(der)
        .map_err(|_| bad_input("This .pfx has a malformed private key."))?;
    let mut reader = SliceReader::new(seq.value())
        .map_err(|_| bad_input("This .pfx has a malformed private key."))?;

    let alg = AlgorithmIdentifierOwned::decode(&mut reader)
        .map_err(|_| bad_input("This .pfx has a malformed private key."))?;
    let ciphertext = OctetString::decode(&mut reader)
        .map_err(|_| bad_input("This .pfx has a malformed private key."))?;
    let alg_der = alg
        .to_der()
        .map_err(|e| CommandError::Internal(format!("pkcs12: re-encoding a scheme: {e}")))?;

    decrypt_with_scheme(&alg_der, ciphertext.as_bytes(), password)
}

/// Pull keys and certificates out of a safe's bags.
fn collect_bags(
    contents: &SafeContents,
    password: &str,
    keys: &mut Vec<RsaPrivateKey>,
    certs: &mut Vec<Certificate>,
) -> Result<(), CommandError> {
    use pkcs8::PrivateKeyInfo;

    for bag in contents {
        // `bag_value` is the raw TLV of the `[0] EXPLICIT` wrapper, not the
        // content. Re-encoding it with `to_der()` would emit a `Vec<u8>` as an
        // OCTET STRING and every bag would then fail to parse — which is
        // exactly what it did. `Any::value()` is the inside of the wrapper.
        let wrapper = der::Any::from_der(&bag.bag_value)
            .map_err(|_| bad_input("This .pfx has a malformed bag."))?;
        let der = wrapper.value();

        match bag.bag_id {
            oid::SHROUDED_KEY_BAG => {
                let plain = decrypt_shrouded_key(der, password)?;
                let pki = PrivateKeyInfo::from_der(&plain)
                    .map_err(|_| bad_input("This .pfx has a private key VibePDF can't read."))?;
                keys.push(rsa_from_pkcs8(&pki)?);
            }
            oid::KEY_BAG => {
                let pki = PrivateKeyInfo::from_der(der)
                    .map_err(|_| bad_input("This .pfx has a private key VibePDF can't read."))?;
                keys.push(rsa_from_pkcs8(&pki)?);
            }
            oid::CERT_BAG => {
                let cert_bag = CertBag::from_der(der)
                    .map_err(|_| bad_input("This .pfx has a malformed certificate."))?;
                let cert = Certificate::from_der(cert_bag.cert_value.as_bytes())
                    .map_err(|_| bad_input("This .pfx has a certificate VibePDF can't read."))?;
                certs.push(cert);
            }
            // secretBag / safeContentsBag — nothing we sign with.
            _ => {}
        }
    }
    Ok(())
}

/// PKCS#8 → RSA. Anything else (EC, Ed25519) is refused by name.
fn rsa_from_pkcs8(pki: &pkcs8::PrivateKeyInfo<'_>) -> Result<RsaPrivateKey, CommandError> {
    use const_oid::db::rfc5912::RSA_ENCRYPTION;
    use rsa::pkcs8::DecodePrivateKey;

    if pki.algorithm.oid != RSA_ENCRYPTION {
        return Err(bad_input(
            "That certificate uses a key type VibePDF can't sign with yet — only RSA is supported.",
        ));
    }
    let der = pki
        .to_der()
        .map_err(|e| CommandError::Internal(format!("pkcs12: re-encoding a key: {e}")))?;
    RsaPrivateKey::from_pkcs8_der(&der)
        .map_err(|_| bad_input("This .pfx has an RSA key VibePDF couldn't parse."))
}

/// The certificate whose public key matches the private one.
///
/// A `.pfx` routinely holds a chain, and picking the wrong end of it produces a
/// signature that verifies against a certificate that did not make it — which
/// every verifier rejects, with a message about the certificate rather than
/// about us. Matching on the public key is the only reliable test; subject
/// names and file order are not.
fn pick_signer(
    key: &RsaPrivateKey,
    certs: &mut Vec<Certificate>,
) -> Result<Certificate, CommandError> {
    use rsa::pkcs1::EncodeRsaPublicKey;
    use rsa::traits::PublicKeyParts;

    let ours = rsa::RsaPublicKey::new(key.n().clone(), key.e().clone())
        .map_err(|e| CommandError::Internal(format!("pkcs12: rebuilding public key: {e}")))?
        .to_pkcs1_der()
        .map_err(|e| CommandError::Internal(format!("pkcs12: encoding public key: {e}")))?;

    let at = certs.iter().position(|c| {
        c.tbs_certificate
            .subject_public_key_info
            .subject_public_key
            .as_bytes()
            .is_some_and(|b| b == ours.as_bytes())
    });

    match at {
        Some(i) => Ok(certs.remove(i)),
        None => Err(bad_input(
            "That .pfx has no certificate matching its private key.",
        )),
    }
}

// SHA-1 and 3DES appear above only to *read* existing certificate files: RFC
// 7292's older KDF and MAC specify them and files in the wild still use them.
// Neither goes anywhere near the signature, which is SHA-256 per `PAdES`.
