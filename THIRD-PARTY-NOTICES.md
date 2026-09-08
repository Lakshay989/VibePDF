# Third-party notices

VibePDF's own source is dual-licensed `MIT OR Apache-2.0` (see
[`COPYRIGHT`](COPYRIGHT)). It bundles and links third-party components that keep
their own terms. Several of those terms are *redistribution* obligations: they
bind a shipped `.dmg` / `.msi` / `.AppImage`, not a source checkout.

> **Status.** The components below are the ones that end up inside a built
> application, and their licences have been verified. A full transitive
> dependency inventory has **not** been produced yet — see
> [Before the first binary release](#before-the-first-binary-release). Nothing
> here is a legal opinion.

## Components in a shipped binary

### PDFium

- **Upstream:** the PDF library from the Chromium project (Google).
- **Obtained as:** a prebuilt binary from
  [`bblanchon/pdfium-binaries`](https://github.com/bblanchon/pdfium-binaries),
  release pinned to `chromium/7857` in `scripts/fetch-pdfium.sh`.
- **Licence:** BSD 3-Clause (PDFium itself), with Apache-2.0 and other terms
  covering its own vendored third-party code — its `LICENSE` and
  `AUTHORS` files travel in the release archive.
- **Obligation:** binary redistribution must reproduce the copyright notice,
  the licence text, and the disclaimer. **PDFium's own `LICENSE` file must be
  copied into the bundle** — this repository does not vendor it, because the
  binary is fetched at build time and is gitignored.

### PDF.js

- **Upstream:** Mozilla, via the `pdfjs-dist` npm package.
- **Licence:** Apache License 2.0.
- **Obligation:** retain the licence and any `NOTICE` file; state changes if the
  source is modified (VibePDF does not modify it — `scripts/copy-pdfjs-worker.mjs`
  copies the worker verbatim).

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
justification comment. Tauri, the RustCrypto crates (`cms`, `x509-cert`, `rsa`,
`pkcs12`, `pkcs5`, `pkcs8`, `sha2`, `der`, …), `lopdf`, `pdfium-render`,
`regex`, `ttf-parser` and `subsetter` are all published under permissive terms,
but **the transitive tree has not been audited**. Do not treat this paragraph as
the inventory.

## Test-only components (not shipped)

### Noto Sans Coptic

- `tests/fixtures/fonts/NotoSansCoptic-Regular.ttf`
- © The Noto Project Authors, SIL Open Font License 1.1.
- Notice: `tests/fixtures/fonts/OFL-NOTICE.txt`.
- Used only to exercise the font-embedding path in tests. Not bundled with the
  application, so no redistribution obligation is triggered today. If a font is
  ever shipped, the OFL notice ships with it.

## Before the first binary release

These are open, and each one blocks distribution rather than development:

1. **Generate a real dependency inventory.** `cargo install cargo-about` (or
   `cargo-license`) for the Rust tree, and a licence checker for the npm tree.
   Commit the output, don't paraphrase it.
2. **Copy PDFium's `LICENSE` and `AUTHORS` into the bundle.** They arrive in the
   release archive that `fetch-pdfium.sh` unpacks and are currently discarded.
3. **Ship a NOTICE for the Apache-2.0 components** (PDF.js, Tauri where it is
   taken under Apache-2.0).
4. **Surface the notices in the application** — an "Open source licences" view,
   or a notices file beside the executable.
5. **Pin the PDFium download by checksum.** `fetch-pdfium.sh` fetches over HTTPS
   with no hash verification, so the build trusts whatever that URL serves. This
   is a supply-chain gap, not a licensing one, but it is fixed in the same file.
