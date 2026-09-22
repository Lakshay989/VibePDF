# Third-party notices

VibePDF's own source is dual-licensed `MIT OR Apache-2.0` (see
[`COPYRIGHT`](COPYRIGHT)). It bundles and links third-party components that keep
their own terms. Several of those terms are *redistribution* obligations: they
bind a shipped `.dmg` / `.msi` / `.AppImage`, not a source checkout.

> **This file is the reasoning; the list is generated.** The full inventory of
> what a build contains lives in
> [`THIRD-PARTY-LICENSES.md`](THIRD-PARTY-LICENSES.md), produced by
> `npm run licenses` from the resolved dependency trees. This file explains the
> components that carry a real obligation and how it is met. Nothing here is a
> legal opinion.

## Components in a shipped binary

### PDFium

- **Upstream:** the PDF library from the Chromium project (Google).
- **Obtained as:** a prebuilt binary from
  [`bblanchon/pdfium-binaries`](https://github.com/bblanchon/pdfium-binaries),
  release pinned to `chromium/7857` in `scripts/fetch-pdfium.sh`.
- **Licence:** BSD 3-Clause for PDFium itself, plus separate terms for the
  libraries it vendors. The archive carries `licenses/pdfium.txt` and a file per
  vendored component (FreeType, ICU, lcms, libjpeg-turbo, libpng, libtiff,
  OpenJPEG, zlib, Abseil, agg23, fast_float, llvm-libc, simdutf), and a root
  `LICENSE` which is the **packaging repository's own MIT licence** (Benoit
  Blanchon) rather than PDFium's — a distinction worth keeping straight, since
  shipping only the root file would attribute the wrong project.
- **Obligation:** binary redistribution must reproduce the copyright notice,
  the licence text, and the disclaimer. **Met:** `scripts/fetch-pdfium.sh`
  copies both the root `LICENSE` and the whole `licenses/` directory into
  `resources/pdfium/`, which `tauri.conf.json` bundles. They are copied rather
  than committed so they stay pinned to the release actually fetched.

### PDF.js

- **Upstream:** Mozilla, via the `pdfjs-dist` npm package.
- **Licence:** Apache License 2.0.
- **Obligation:** retain the licence and any `NOTICE` file; state changes if the
  source is modified (VibePDF does not modify it — `scripts/copy-pdfjs-worker.mjs`
  copies the worker and data files verbatim).

### PDF.js image decoders

PDF.js decodes JBIG2, CCITT fax and JPEG 2000 images, and applies ICC colour,
with WebAssembly modules that ship inside `pdfjs-dist` but are built from other
projects. `scripts/copy-pdfjs-worker.mjs` serves them from `public/pdfjs/`, so
they are in every build.

| Component | Module | Licence | Licence file shipped beside it |
|---|---|---|---|
| PDFium's JBIG2 / CCITT decoder | `wasm/jbig2.wasm` | BSD-3-Clause (Mozilla glue: Apache-2.0) | `LICENSE_JBIG2`, `LICENSE_PDFJS_JBIG2` |
| OpenJPEG | `wasm/openjpeg.wasm` | BSD-2-Clause (glue: BSD-2-Clause) | `LICENSE_OPENJPEG`, `LICENSE_PDFJS_OPENJPEG` |
| qcms | `wasm/qcms_bg.wasm` | MIT (glue: MIT) | `LICENSE_QCMS`, `LICENSE_PDFJS_QCMS` |
| CGATS001Compat-v2-micro ICC profile | `iccs/CGATS001Compat-v2-micro.icc` | CC0-1.0 | `iccs/LICENSE` |

- **Obligation:** the BSD and MIT terms require the copyright notice and
  licence text to accompany binary redistribution. **Met:** the copy script
  copies each licence file into the same directory as its module, and the
  build bundles that directory whole. The same components are listed in the
  generated inventory and the in-app Licences view.
- **Not shipped:** `wasm/quickjs-eval.*`, PDF.js's sandbox for a document's own
  JavaScript. The copy script excludes it, and a test fails if it is served.

### Tesseract OCR (P7)

The OCR engine and the image library under it are compiled from source into the
binary; the English model ships beside the executable as a Tauri resource.

| Component | Version | Licence | How it arrives |
|---|---|---|---|
| [Tesseract](https://github.com/tesseract-ocr/tesseract) | 5.5.2 | Apache-2.0 | Built from a checksum-pinned source archive by `scripts/fetch-tesseract-src.sh`, statically linked |
| [Leptonica](https://github.com/DanBloomberg/leptonica) | 1.87.0 | BSD-2-Clause | Same archive step; Tesseract's image layer |
| [Twelve language models](https://github.com/tesseract-ocr/tessdata_fast) (eng, spa, fra, deu, chi_sim, chi_tra, jpn, kor, ara, hin, por, rus) | pinned commit | Apache-2.0 | `scripts/fetch-tessdata.sh` → `src-tauri/resources/tessdata/`, bundled with the repository's `LICENSE` beside them |

- **Obligation:** Apache-2.0 needs the licence retained and attribution kept
  (§4(d)); BSD-2-Clause needs the copyright notice and disclaimer with binary
  redistribution. **Met:** Tesseract's attribution is in [`NOTICE`](NOTICE), the
  language model's `LICENSE` is downloaded next to it into
  `resources/tessdata/`, and both libraries' licence files sit in the pinned
  source archives the build compiles.
- **Still open:** the statically linked libraries' licence texts are not yet
  copied into the bundle the way PDFium's are. A binary release must do that;
  `fetch-tesseract-src.sh` is where it belongs.

### Frontend runtime dependencies

Verified from the installed package metadata. All permissive; no copyleft.

| Package | Licence |
|---|---|
| `react`, `react-dom` | MIT |
| `zustand` | MIT |
| `@radix-ui/react-dialog`, `-dropdown-menu`, `-tooltip` | MIT |
| `@tauri-apps/api` | Apache-2.0 OR MIT |
| `@tauri-apps/plugin-dialog`, `-fs` | MIT OR Apache-2.0 |
| `pdfjs-dist` | Apache-2.0 |

### Rust dependencies

The direct dependencies are declared in `src-tauri/Cargo.toml`, each with its
justification comment. The full transitive set — 388 crates that actually link
on at least one of the five target triples we release for, as `cargo tree`
resolves them — is enumerated in
[`THIRD-PARTY-LICENSES.md`](THIRD-PARTY-LICENSES.md).

Five of them are MPL-2.0, all in Tauri's WebView stack. MPL-2.0 is *file-level*
copyleft: the obligation attaches to the MPL-licensed files and only if they are
modified, which we do not do. That is the single place `docs/01_VISION.md`'s "no
copyleft in the shipped binary" is interpreted rather than applied literally, and
it is written down in the generated file too so the decision stays visible.

## Test-only components (not shipped)

### Noto Sans Coptic

- `tests/fixtures/fonts/NotoSansCoptic-Regular.ttf`
- © The Noto Project Authors, SIL Open Font License 1.1.
- Notice: `tests/fixtures/fonts/OFL-NOTICE.txt`.
- Used only to exercise the font-embedding path in tests. Not bundled with the
  application, so no redistribution obligation is triggered today. If a font is
  ever shipped, the OFL notice ships with it.

## Release obligations

All five are now met. They are recorded here because each one is a thing a
*binary* distribution owes and a source checkout does not, so they are easy to
forget until someone asks for a build.

1. **A real dependency inventory.** Generated, not written:
   `npm run licenses` reads `cargo tree` per target triple and `package-lock.json` into
   [`THIRD-PARTY-LICENSES.md`](THIRD-PARTY-LICENSES.md) — 388 Rust crates,
   62 npm packages, 9 bundled components, dev- and build-only dependencies
   excluded because they ship to nobody. The same script is a gate: it exits
   non-zero if any shipped component carries a licence outside a reviewed
   permissive set, which is where `docs/01_VISION.md`'s "no copyleft in the
   shipped binary" is checked rather than assumed.
2. **PDFium's licences travel with the binary.** `scripts/fetch-pdfium.sh`
   copies the archive's `LICENSE` and its whole `licenses/` directory —
   PDFium's own BSD-3-Clause plus a file per vendored component (FreeType, ICU,
   libjpeg-turbo, libpng, libtiff, OpenJPEG, zlib, Abseil and others) — into
   `resources/pdfium/`, which `tauri.conf.json` bundles. Copied rather than
   committed so they stay pinned to the release actually fetched.
3. **A NOTICE for the Apache-2.0 components.** [`NOTICE`](NOTICE) carries the
   attributions Apache-2.0 §4(d) requires. None of the upstream projects ships
   a NOTICE of its own today; if one appears, its contents belong there verbatim.
4. **The notices are surfaced in the application.** A "Licences" button in the
   toolbar opens the generated inventory, available whether or not a document is
   open. A file beside the executable satisfies the letter of the obligation and
   is read by nobody.
5. **The PDFium download is pinned by checksum.** `fetch-pdfium.sh` verifies
   each asset against a SHA-256 taken from the SLSA provenance upstream signs
   for the release, and refuses to unpack anything that does not match.

### Still open

- **Windows.** `fetch-pdfium.sh` has no Windows branch, so the obligations above
  are satisfied on macOS and Linux only. A Windows build must copy the same
  licence files.
- The inventory is not regenerated automatically. `npm run licenses -- --check`
  reports staleness; nothing runs it in CI yet.
