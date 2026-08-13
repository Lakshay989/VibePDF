#!/usr/bin/env bash
# Regenerate the P6.B1 signing fixtures.
#
# A self-signed test certificate and two PKCS#12 wrappings of it. Nothing here
# is a secret: the key is in the repo on purpose so the signing tests are
# deterministic and run offline. It is self-signed and expires, so it can never
# be trusted by anything real — which is the point.
#
# The two .pfx flavours exist because they are genuinely different files:
#
#   signer.pfx         OpenSSL 3 default — PBES2/AES-256-CBC, HMAC-SHA256 MAC.
#                      What anyone generating a certificate today gets.
#   signer-legacy.pfx  PKCS#12 PBE with SHA-1/3DES, SHA-1 MAC. What a decade of
#                      existing files look like, and still what some CAs issue.
#
# A reader that handles only one of these looks correct until it meets the
# other. `p12` 0.6.3 panics on the first; a PBES2-only implementation fails the
# second.
#
# Requires openssl (3.x). Run from anywhere:
#     bash tests/fixtures/certs/generate-test-cert.sh
set -euo pipefail

cd "$(dirname "$0")"

PASSWORD="test123"
SUBJECT="/CN=VibePDF Test Signer/O=VibePDF/C=GB"

openssl req -x509 -newkey rsa:2048 \
  -keyout signer-key.pem -out signer-cert.pem \
  -days 3650 -nodes -sha256 -subj "$SUBJECT" 2>/dev/null

# OpenSSL 3 defaults: PBES2 + AES-256-CBC, MAC over SHA-256.
openssl pkcs12 -export -out signer.pfx \
  -inkey signer-key.pem -in signer-cert.pem \
  -name "VibePDF Test Signer" -passout "pass:$PASSWORD"

# The older shape, still common in the wild.
openssl pkcs12 -export -out signer-legacy.pfx \
  -inkey signer-key.pem -in signer-cert.pem \
  -name "VibePDF Test Signer" -passout "pass:$PASSWORD" \
  -certpbe PBE-SHA1-3DES -keypbe PBE-SHA1-3DES -macalg sha1

echo "wrote signer.pfx, signer-legacy.pfx, signer-cert.pem, signer-key.pem (password: $PASSWORD)"
