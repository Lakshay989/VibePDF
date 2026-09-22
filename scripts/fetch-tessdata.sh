#!/usr/bin/env bash
# Fetches the OCR language data VibePDF ships into
# src-tauri/resources/tessdata/, which tauri.conf.json bundles. Same shape as
# fetch-pdfium.sh: too big to commit (~25 MB), pinned by checksum, verified
# before it lands.
#
# SPEC: P7-OCR-002 names twelve languages the application must support. All
# twelve ship, so OCR works with no network at all; anything beyond them the
# user adds from a file or downloads deliberately (src-tauri/src/ocr/languages.rs).
#
# `src-tauri/src/ocr/tessdata.rs` resolves this directory at runtime.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="$REPO_ROOT/src-tauri/resources/tessdata"

# tessdata_fast: the integer-quantised LSTM models Tesseract recommends as the
# default. "best" is ~3x larger for a small accuracy gain; "legacy" needs the
# pre-LSTM engine we don't use.
#
# Pinned to a commit rather than a branch: `main` moves, and a language model is
# exactly the kind of large binary nobody would notice changing. The digests are
# ours, taken from this download and recorded here — upstream publishes no signed
# provenance for these files, so this pins *continuity* (the bytes never change
# silently), not upstream identity.
#
# To add or bump a language: change the list, run with
# VIBEPDF_TESSDATA_ALLOW_NEW=1 to print the digests, paste them back, re-run.
TESSDATA_COMMIT="87416418657359cb625c412a48b6e1d6d41c29bd"

LANGUAGES=(eng spa fra deu chi_sim chi_tra jpn kor ara hin por rus)

SHA256_eng="7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2"
SHA256_spa="6f2e04d02774a18f01bed44b1111f2cd7f3ba7ac9dc4373cd3f898a40ea6b464"
SHA256_fra="ced037562e8c80c13122dece28dd477d399af80911a28791a66a63ac1e3445ca"
SHA256_deu="19d219bbb6672c869d20a9636c6816a81eb9a71796cb93ebe0cb1530e2cdb22d"
SHA256_chi_sim="a5fcb6f0db1e1d6d8522f39db4e848f05984669172e584e8d76b6b3141e1f730"
SHA256_chi_tra="529c5b5797d64b126065cd55f2bb4c7fd7b15790798091b1ff259941a829330b"
SHA256_jpn="1f5de9236d2e85f5fdf4b3c500f2d4926f8d9449f28f5394472d9e8d83b91b4d"
SHA256_kor="6b85e11d9bbf07863b97b3523b1b112844c43e713df8b66418a081fd1060b3b2"
SHA256_ara="e3206d3dc87fd50c24a0fb9f01838615911d25168f4e64415244b67d2bb3e729"
SHA256_hin="4c73ffc59d497c186b19d1e90f5d721d678ea6b2e277b719bee4e2af12271825"
SHA256_por="c4932b937207a9514b7514d518b931a99938c02a28a5a5a553f8599ed58b7deb"
SHA256_rus="e16e5e036cce1d9ec2b00063cf8b54472625b9e14d893a169e2b0dedeb4df225"

RAW="https://raw.githubusercontent.com/tesseract-ocr/tessdata_fast/$TESSDATA_COMMIT"

mkdir -p "$DEST"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "fetch-tessdata: ${#LANGUAGES[@]} languages @ ${TESSDATA_COMMIT:0:12}"
for lang in "${LANGUAGES[@]}"; do
  expected_var="SHA256_$lang"
  expected="${!expected_var:-}"
  target="$DEST/$lang.traineddata"

  if [ -f "$target" ] && [ "$(shasum -a 256 "$target" | cut -d' ' -f1)" = "$expected" ]; then
    echo "  $lang — already present"
    continue
  fi

  curl -fsSL -o "$tmp/$lang.traineddata" "$RAW/$lang.traineddata"
  actual="$(shasum -a 256 "$tmp/$lang.traineddata" | cut -d' ' -f1)"
  if [ "${VIBEPDF_TESSDATA_ALLOW_NEW:-}" = "1" ]; then
    echo "  SHA256_$lang=\"$actual\""
  elif [ "$actual" != "$expected" ]; then
    echo "fetch-tessdata: checksum mismatch for $lang.traineddata" >&2
    echo "  expected $expected" >&2
    echo "  actual   $actual" >&2
    echo "  Refusing to install it. If this is a deliberate bump, re-run with VIBEPDF_TESSDATA_ALLOW_NEW=1." >&2
    exit 1
  fi
  mv "$tmp/$lang.traineddata" "$target"
  echo "  $lang — $(du -h "$target" | cut -f1)"
done

# Apache-2.0, same licence as Tesseract itself; it ships beside the data so the
# obligation travels with the files (THIRD-PARTY-NOTICES.md § Tesseract).
curl -fsSL -o "$DEST/LICENSE" "$RAW/LICENSE"

echo "fetch-tessdata: $(du -sh "$DEST" | cut -f1) in $DEST"
