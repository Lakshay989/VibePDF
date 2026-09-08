# Test signing credentials — NOT SECRETS, NEVER TRUST

**The private keys in this directory are committed on purpose.** They are test
fixtures. If a secret scanner brought you here, this is a true positive for
"a private key is in the repository" and a false positive for "a credential
leaked".

| File | What it is |
|---|---|
| `signer-key.pem` | RSA-2048 private key, unencrypted |
| `signer-cert.pem` | Self-signed certificate, `CN=VibePDF Test Signer, O=VibePDF, C=GB` |
| `signer.pfx` | PKCS#12 of the pair — OpenSSL 3 defaults (PBES2/AES-256-CBC, HMAC-SHA256) |
| `signer-legacy.pfx` | Same pair — legacy PKCS#12 PBE (SHA-1/3DES, SHA-1 MAC) |
| `generate-test-cert.sh` | Regenerates all four |

Both `.pfx` files use the password **`test123`**, which is written in the
generator script and in the tests.

## Why they are committed

The signing suite must be deterministic and must run offline — those are
project-wide constraints, not conveniences. Generating a key per test run would
make failures irreproducible; fetching one would put a network call in the test
path.

The two PKCS#12 flavours exist because they are genuinely different container
formats, and an implementation that handles one silently fails the other. That
divergence is the thing under test. See the header comment in
`generate-test-cert.sh`.

## Why it is safe

The certificate is **self-signed**. It chains to no trust anchor, so no PDF
reader, OS trust store, or CA will ever accept a signature made with it — a
document signed by this key verifies as "signed by an unknown issuer" and
nothing more. That is exactly what the verification tests assert
(`ChainStatus::SelfSigned` in `src-tauri/src/security/verify.rs`, which has no
`Trusted` variant at all).

## Rules

- **Never use these for anything real.** Not for a demo signature you intend
  anyone to believe, not as a template with the subject edited.
- **Never copy this pattern into code that handles a user's real credential.**
  `src-tauri/src/security/credential.rs` never logs, prints, or serialises key
  material; its `Debug` impl is hand-written to keep the key out of any log line.
- If you need a credential for manual testing, generate your own with
  `generate-test-cert.sh` and keep it in the gitignored `Sample PDFs/` tree.
