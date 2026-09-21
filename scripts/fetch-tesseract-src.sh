#!/usr/bin/env bash
# Places checksum-verified Tesseract and Leptonica sources where the
# `tesseract-rs` crate's build script expects them, so that build never
# downloads anything itself.
#
# Why this exists: tesseract-rs compiles both libraries from source (that is
# what buys us "nothing for the user to install"), and its build.rs fetches
# two GitHub archives with no integrity check. It does, however, reuse
# `<cache>/third_party/{leptonica,tesseract}` when those directories already
# exist. This script fills them from the same pinned URLs, verified first —
# the same posture as fetch-pdfium.sh, applied to somebody else's build script.
#
# Run once per machine, before the first `cargo build`. Re-running is cheap: it
# skips work when the sources are already in place.
set -euo pipefail

# Must match the versions in tesseract-rs's build.rs. If the crate is bumped
# and these drift, its build.rs downloads its own copy instead of using these,
# which the verify step below catches.
LEPTONICA_VERSION="1.87.0"
TESSERACT_VERSION="5.5.2"
SHA256_leptonica="2cfb7ebe6036f3d017280fa9a000c9699ab28be8dff8be8e2e948e2c79917eda"
SHA256_tesseract="449621680d529fde7a2d079805dce4687fef0a692c272be3ce431c30c69162d2"

# Where the crate keeps its build cache, mirroring get_custom_out_dir() in its
# build.rs.
case "$(uname -s)" in
  Darwin) CACHE="$HOME/Library/Application Support/tesseract-rs" ;;
  Linux|FreeBSD) CACHE="$HOME/.tesseract-rs" ;;
  *) echo "fetch-tesseract-src: unsupported OS $(uname -s) (Windows: %APPDATA%\\tesseract-rs)" >&2; exit 1 ;;
esac
THIRD_PARTY="$CACHE/third_party"
mkdir -p "$THIRD_PARTY"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

fetch() {
  name="$1"; url="$2"; expected="$3"; stripped="$4"
  if [ -d "$THIRD_PARTY/$name" ]; then
    echo "fetch-tesseract-src: $name already present — leaving it alone"
    return
  fi
  echo "fetch-tesseract-src: $name"
  curl -fsSL -o "$tmp/$name.zip" "$url"
  actual="$(shasum -a 256 "$tmp/$name.zip" | cut -d' ' -f1)"
  if [ "$actual" != "$expected" ]; then
    echo "fetch-tesseract-src: checksum mismatch for $name" >&2
    echo "  expected $expected" >&2
    echo "  actual   $actual" >&2
    exit 1
  fi
  unzip -q "$tmp/$name.zip" -d "$tmp/$name-extract"
  mv "$tmp/$name-extract/$stripped" "$THIRD_PARTY/$name"
}

fetch leptonica \
  "https://github.com/DanBloomberg/leptonica/archive/refs/tags/$LEPTONICA_VERSION.zip" \
  "$SHA256_leptonica" "leptonica-$LEPTONICA_VERSION"
fetch tesseract \
  "https://github.com/tesseract-ocr/tesseract/archive/refs/tags/$TESSERACT_VERSION.zip" \
  "$SHA256_tesseract" "tesseract-$TESSERACT_VERSION"

# The crate's build.rs also insists on `<cache>/tessdata/{eng,tur}.traineddata`,
# downloading them from tessdata_best with no integrity check and failing the
# whole build if the network is slow — even though nothing reads them unless
# the `embed-tessdata` feature is on, which it isn't here. Pre-placing verified
# copies makes that step a no-op, so a build never reaches for the network.
#
# These are *not* the models the app uses: `src-tauri/resources/tessdata`, from
# fetch-tessdata.sh, is what ships and what `ocr::tessdata` resolves. The
# Turkish file is here only because the build script demands the name.
TESSDATA_COMMIT="87416418657359cb625c412a48b6e1d6d41c29bd"
SHA256_tessdata_eng="7d4322bd2a7749724879683fc3912cb542f19906c83bcc1a52132556427170b2"
SHA256_tessdata_tur="7393381111e1152420fc4092cb44eef4237580d21b92bf30d7d221aad192c6b7"

place_tessdata() {
  lang="$1"; expected="$2"
  target="$CACHE/tessdata/$lang.traineddata"
  mkdir -p "$CACHE/tessdata"
  if [ -f "$target" ] && [ "$(shasum -a 256 "$target" | cut -d' ' -f1)" = "$expected" ]; then
    echo "fetch-tesseract-src: build-cache $lang.traineddata already verified"
    return
  fi
  echo "fetch-tesseract-src: build-cache $lang.traineddata"
  curl -fsSL -o "$tmp/$lang.traineddata" \
    "https://raw.githubusercontent.com/tesseract-ocr/tessdata_fast/$TESSDATA_COMMIT/$lang.traineddata"
  actual="$(shasum -a 256 "$tmp/$lang.traineddata" | cut -d' ' -f1)"
  if [ "$actual" != "$expected" ]; then
    echo "fetch-tesseract-src: checksum mismatch for $lang.traineddata" >&2
    echo "  expected $expected" >&2
    echo "  actual   $actual" >&2
    exit 1
  fi
  mv "$tmp/$lang.traineddata" "$target"
}

place_tessdata eng "$SHA256_tessdata_eng"
place_tessdata tur "$SHA256_tessdata_tur"

for dir in leptonica tesseract; do
  if [ ! -f "$THIRD_PARTY/$dir/CMakeLists.txt" ]; then
    echo "fetch-tesseract-src: $THIRD_PARTY/$dir has no CMakeLists.txt — the build would refetch it" >&2
    exit 1
  fi
done

echo "fetch-tesseract-src: sources ready in $THIRD_PARTY"
echo "  leptonica $LEPTONICA_VERSION, tesseract $TESSERACT_VERSION (verified)"
