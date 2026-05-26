#!/usr/bin/env bash
# Fetches a prebuilt PDFium binary from bblanchon/pdfium-binaries and
# drops it into src-tauri/resources/pdfium/. The Rust side's
# `Pdfium::bind_to_system_library` walks the standard search path, which
# on macOS includes `@executable_path` — Tauri places bundled resources
# there at install time. For `tauri dev`, you may need to additionally
# `export DYLD_LIBRARY_PATH="$PWD/src-tauri/resources/pdfium:$DYLD_LIBRARY_PATH"`.
#
# Phase 1 bootstrap: macOS only (Intel + Apple Silicon). Linux + Windows
# branches land before Phase 1 ships.
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
PDFIUM_RELEASE="${PDFIUM_RELEASE:-chromium/7857}"

uname_s="$(uname -s)"
uname_m="$(uname -m)"

case "$uname_s-$uname_m" in
  Darwin-arm64)  ASSET="pdfium-mac-arm64.tgz" ;;
  Darwin-x86_64) ASSET="pdfium-mac-x64.tgz" ;;
  Linux-x86_64)  ASSET="pdfium-linux-x64.tgz" ;;
  Linux-aarch64) ASSET="pdfium-linux-arm64.tgz" ;;
  *)
    echo "fetch-pdfium: unsupported platform: $uname_s-$uname_m" >&2
    exit 1
    ;;
esac

URL="https://github.com/bblanchon/pdfium-binaries/releases/download/${PDFIUM_RELEASE}/${ASSET}"
echo "fetch-pdfium: $URL"

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT
curl -fL --retry 3 -o "$tmpdir/pdfium.tgz" "$URL"
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
