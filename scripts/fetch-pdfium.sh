#!/usr/bin/env bash
# Fetches a prebuilt PDFium binary from bblanchon/pdfium-binaries and
# drops it into src-tauri/resources/pdfium/. The Rust side's
# `Pdfium::bind_to_system_library` walks the standard search path, which
# on macOS includes `@executable_path` — Tauri places bundled resources
# there at install time. For `tauri dev`, you may need to additionally
# `export DYLD_LIBRARY_PATH="$PWD/src-tauri/resources/pdfium:$DYLD_LIBRARY_PATH"`.
#
# macOS (Intel + Apple Silicon) and Linux (x64 + arm64). No Windows branch
# yet — see the note in .github/workflows/ci.yml on the check-windows job.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="$REPO_ROOT/src-tauri/resources/pdfium"
mkdir -p "$DEST"

# Pinned PDFium build. bblanchon/pdfium-binaries cuts a release roughly
# weekly; we pin one to keep CI deterministic. Update by bumping this
# value and verifying with the PDFium smoke test.
#
# 2026-05-25: bumped from chromium/6996 → chromium/7857. The previous
# pin pre-dated pdfium-render 0.9.1, which calls symbols
# (e.g. FPDF_StructElement_GetExpansion) that landed in PDFium >=7000.
PINNED_RELEASE="chromium/7857"
PDFIUM_RELEASE="${PDFIUM_RELEASE:-$PINNED_RELEASE}"

# SHA-256 of each release asset, checked before anything is unpacked.
#
# This is a native shared library that gets linked into an application whose
# entire pitch is that it can be trusted with your documents. Without a hash the
# build trusts whatever that URL serves at the moment CI runs it — a tag can be
# re-pointed, a release asset can be replaced, and neither leaves a trace in
# this repository.
#
# The digests are not ours: they come from the SLSA v1 provenance that
# bblanchon/pdfium-binaries publishes as `pdfium-attestation.json` on each
# release, signed through Sigstore by its own build workflow. They were checked
# against a real download and against `gh attestation verify` before being
# committed here.
#
# What this does and does not buy: it proves the bytes are the same bytes the
# upstream build produced for this release, and it fails loudly if they ever
# change. It is not a review of PDFium, and it does not make a compromised
# upstream build safe. It makes substitution detectable, which is the part a
# build script can actually do.
#
# To regenerate after bumping PINNED_RELEASE:
#
#   curl -fsSL "https://github.com/bblanchon/pdfium-binaries/releases/download/\
#   ${PINNED_RELEASE/\//%2F}/pdfium-attestation.json" \
#     | python3 -c 'import sys,json,base64; \
#       p=json.loads(base64.b64decode(json.load(sys.stdin)["dsseEnvelope"]["payload"])); \
#       [print(s["name"], s["digest"]["sha256"]) for s in p["subject"]]'
#
# and cross-check one of them with:
#
#   gh attestation verify <downloaded.tgz> --repo bblanchon/pdfium-binaries
#
SHA256_pdfium_mac_arm64="65a4a6b0028675113cac99cad61469eb6a482d7283e21a1faefc6e63587109c5"
SHA256_pdfium_mac_x64="b99913c97f7fb357e69c0ab130012827c60054bc5c9d318afc2c7fd36ccd2c70"
SHA256_pdfium_linux_x64="2ad1fd4237cd491201ac74a72388199b9dcf546c5cb02d8fea700725a1b80541"
SHA256_pdfium_linux_arm64="0e24373e73c50759136196c0078db8656860c8d03a10b2cb4a2e7b72d8068e35"

uname_s="$(uname -s)"
uname_m="$(uname -m)"

case "$uname_s-$uname_m" in
  Darwin-arm64)  ASSET="pdfium-mac-arm64.tgz";   EXPECTED="$SHA256_pdfium_mac_arm64" ;;
  Darwin-x86_64) ASSET="pdfium-mac-x64.tgz";     EXPECTED="$SHA256_pdfium_mac_x64" ;;
  Linux-x86_64)  ASSET="pdfium-linux-x64.tgz";   EXPECTED="$SHA256_pdfium_linux_x64" ;;
  Linux-aarch64) ASSET="pdfium-linux-arm64.tgz"; EXPECTED="$SHA256_pdfium_linux_arm64" ;;
  *)
    echo "fetch-pdfium: unsupported platform: $uname_s-$uname_m" >&2
    exit 1
    ;;
esac

# The digests above describe exactly one release, so an override invalidates
# them. Refuse rather than skip: a verification step that silently turns itself
# off is worse than none, because the build still looks verified.
if [ "$PDFIUM_RELEASE" != "$PINNED_RELEASE" ]; then
  if [ "${PDFIUM_ALLOW_UNPINNED:-0}" = "1" ]; then
    echo "fetch-pdfium: WARNING — $PDFIUM_RELEASE is not the pinned $PINNED_RELEASE." >&2
    echo "fetch-pdfium: WARNING — checksum verification is DISABLED for this run." >&2
    EXPECTED=""
  else
    echo "fetch-pdfium: refusing to fetch $PDFIUM_RELEASE." >&2
    echo "  The committed checksums describe $PINNED_RELEASE only." >&2
    echo "  To bump the pin: edit PINNED_RELEASE and the SHA256_* values (see the" >&2
    echo "  regeneration command in this file's comments)." >&2
    echo "  To try a build without verifying it: PDFIUM_ALLOW_UNPINNED=1" >&2
    exit 1
  fi
fi

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | cut -d' ' -f1
  else
    echo "fetch-pdfium: no sha256sum or shasum on PATH; cannot verify" >&2
    return 1
  fi
}

URL="https://github.com/bblanchon/pdfium-binaries/releases/download/${PDFIUM_RELEASE}/${ASSET}"
echo "fetch-pdfium: $URL"

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT
curl -fL --retry 3 -o "$tmpdir/pdfium.tgz" "$URL"

# Before `tar`, not after: unpacking is the first thing that acts on the
# downloaded bytes, so the check has to come in front of it.
if [ -n "$EXPECTED" ]; then
  actual="$(sha256_of "$tmpdir/pdfium.tgz")"
  if [ "$actual" != "$EXPECTED" ]; then
    echo "fetch-pdfium: CHECKSUM MISMATCH — refusing to install." >&2
    echo "  asset:    $ASSET ($PDFIUM_RELEASE)" >&2
    echo "  expected: $EXPECTED" >&2
    echo "  actual:   $actual" >&2
    echo "  The release asset does not match the digest committed in this script." >&2
    echo "  Do not work around this by editing the digest. Either the upstream" >&2
    echo "  release was modified, or something between here and it changed the" >&2
    echo "  bytes. Check the upstream provenance first:" >&2
    echo "    gh attestation verify <file> --repo bblanchon/pdfium-binaries" >&2
    exit 1
  fi
  echo "fetch-pdfium: sha256 ok ($actual)"
fi

tar -xzf "$tmpdir/pdfium.tgz" -C "$tmpdir"

case "$uname_s" in
  Darwin)
    cp "$tmpdir/lib/libpdfium.dylib" "$DEST/"
    install_name_tool -id "@rpath/libpdfium.dylib" "$DEST/libpdfium.dylib" 2>/dev/null || true
    ;;
  Linux)
    cp "$tmpdir/lib/libpdfium.so" "$DEST/"
    ;;
esac

echo "fetch-pdfium: installed to $DEST"
ls -la "$DEST"
