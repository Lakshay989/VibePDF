//! Reading a PKCS#12 certificate (P6.B1a, SPEC P6-SEC-005).

use std::path::PathBuf;

use vibepdf_lib::security::credential::load_pkcs12;

fn cert(name: &str) -> Vec<u8> {
    let p = PathBuf::from("../tests/fixtures/certs").join(name);
    assert!(
        p.is_file(),
        "fixture missing at {} — run tests/fixtures/certs/generate-test-cert.sh",
        p.display()
    );
    std::fs::read(p).expect("read fixture")
}

// Both flavours, because they are genuinely different files and a reader that
// handles one looks correct until it meets the other. `p12` 0.6.3 panics on the
// first of these; a PBES2-only implementation fails the second.
#[test]
fn opens_a_modern_pfx() {
    let c = load_pkcs12(&cert("signer.pfx"), "test123").expect("modern .pfx");
    assert_eq!(
        c.signer.tbs_certificate.subject.to_string(),
        "C=GB,O=VibePDF,CN=VibePDF Test Signer"
    );
}

#[test]
fn opens_a_legacy_pfx() {
    let c = load_pkcs12(&cert("signer-legacy.pfx"), "test123").expect("legacy .pfx");
    assert_eq!(
        c.signer.tbs_certificate.subject.to_string(),
        "C=GB,O=VibePDF,CN=VibePDF Test Signer"
    );
}

#[test]
fn both_flavours_yield_the_same_credential() {
    // Same key and certificate, two wrappings. If the two paths disagree, one
    // of them is decrypting to something that merely parses.
    let a = load_pkcs12(&cert("signer.pfx"), "test123").expect("modern");
    let b = load_pkcs12(&cert("signer-legacy.pfx"), "test123").expect("legacy");
    assert_eq!(a.signer, b.signer);
    assert_eq!(a.key, b.key);
}

#[test]
fn a_wrong_password_is_refused_and_says_so() {
    let err = load_pkcs12(&cert("signer.pfx"), "not-the-password").unwrap_err();
    let msg = format!("{err:?}");
    assert!(msg.contains("password"), "unhelpful error: {msg}");
}

#[test]
fn a_file_that_is_not_a_pfx_is_refused() {
    let err = load_pkcs12(b"%PDF-1.7\nthis is not a certificate", "test123").unwrap_err();
    assert!(format!("{err:?}").contains("PKCS#12"));
}

// The key must never reach a log or a test failure through Debug.
#[test]
fn debug_does_not_print_the_private_key() {
    let c = load_pkcs12(&cert("signer.pfx"), "test123").expect("load");
    let shown = format!("{c:?}");
    assert!(shown.contains("VibePDF Test Signer"));
    assert!(!shown.contains("RsaPrivateKey"), "key material in Debug: {shown}");
    assert!(!shown.to_lowercase().contains("prime"), "key material in Debug: {shown}");
}
