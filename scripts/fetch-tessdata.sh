#!/usr/bin/env bash
# Fetches Tesseract's English language data into
# src-tauri/resources/tessdata/, which tauri.conf.json bundles. Same shape as
# fetch-pdfium.sh: too big to commit (4 MB), pinned by checksum, verified
# before it lands.
#
# `src-tauri/src/ocr/tessdata.rs` resolves this directory at runtime. Other
# languages are P7.A3's on-demand downloader; this script is the one language
# every build ships.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="$REPO_ROOT/src-tauri/resources/tessdata"

# tessdata_fast: the integer-quantised LSTM models Tesseract recommends as the
# default. "best" is ~3x larger and slower for a small accuracy gain; "legacy"
# needs the pre-LSTM engine we don't use.
#
# Pinned to a commit rather than a branch: `main` moves, and a language model
# is exactly the kind of large binary nobody would notice changing. The digest
# is ours, taken from this download and recorded here — upstream publishes no
# signed provenance for these files, so this pins *continuity* (the bytes never
# change silently), not upstream identity.
#
# To bump: change the commit, run with VIBEPDF_TESSDATA_ALLOW_NEW=1 to print
# the new digests, then paste them back here and re-run without it.
TESSDATA_COMMIT="87416418657359cb625c412a48b6e1d6d41c29bd"
SHA256_eng_traineddata="7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2"

RAW="https://raw.githubusercontent.com/tesseract-ocr/tessdata_fast/$TESSDATA_COMMIT"

mkdir -p "$DEST"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "fetch-tessdata: eng.traineddata @ ${TESSDATA_COMMIT:0:12}"
curl -fsSL -o "$tmp/eng.traineddata" "$RAW/eng.traineddata"
curl -fsSL -o "$tmp/LICENSE" "$RAW/LICENSE"

actual="$(shasum -a 256 "$tmp/eng.traineddata" | cut -d' ' -f1)"
if [ "${VIBEPDF_TESSDATA_ALLOW_NEW:-}" = "1" ]; then
  echo "fetch-tessdata: SHA256_eng_traineddata=\"$actual\""
elif [ "$actual" != "$SHA256_eng_traineddata" ]; then
  echo "fetch-tessdata: checksum mismatch for eng.traineddata" >&2
  echo "  expected $SHA256_eng_traineddata" >&2
  echo "  actual   $actual" >&2
  echo "  Refusing to install it. If this is a deliberate bump, re-run with VIBEPDF_TESSDATA_ALLOW_NEW=1." >&2
  exit 1
fi

mv "$tmp/eng.traineddata" "$DEST/eng.traineddata"
# Apache-2.0, same licence as Tesseract itself; it ships beside the data so the
# obligation travels with the file (THIRD-PARTY-NOTICES.md § OCR).
mv "$tmp/LICENSE" "$DEST/LICENSE"

echo "fetch-tessdata: installed $(du -h "$DEST/eng.traineddata" | cut -f1) → $DEST"
