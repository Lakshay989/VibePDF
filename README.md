# VibePDF

An offline, free PDF editor for Windows, macOS and Linux.

Most capable PDF editors are paywalled, rate-limited, or upload your documents
to somebody else's server. VibePDF aims at the same core surface people pay for
— editing existing text, forms, signatures, encryption, redaction — with no
account, no subscription, no watermark, and no network request you did not ask
for.

**Nothing leaves your machine.** There is no telemetry and no cloud component.
The only network access in the codebase is an optional, off-by-default local
Ollama integration for the AI features, and a build-time script that downloads
the PDFium binary.

---

## Status

Under active development. Not yet released, and there are no binaries to
download — building from source is currently the only way to run it.

| Phase | Scope | State |
|---|---|---|
| 1 | Open, view, navigate, search | Built |
| 2 | Page operations — merge, split, reorder, rotate, crop | Built |
| 3 | Annotations — highlight, notes, ink, shapes, stamps | Built |
| 4 | Content editing — text, images, watermarks, headers | Built |
| 5 | Forms — fill, create, export, flatten | Built |
| 6 | Signing, encryption, redaction | Code complete, in verification |
| 7 | OCR and export to word-processor and spreadsheet formats | Not started |
| 8 | Local AI and batch processing | Not started |

"Built" means the phase's features are implemented, tested, and have passed a
manual verification pass. Phase 6 is implemented and its automated suites are
green, but the cross-reader checks that matter for cryptography and redaction —
opening the output in several independent PDF readers — are still outstanding.
See [`steps/P6-SWEEP.md`](steps/P6-SWEEP.md).

## What works today

**Viewing** — continuous scroll, zoom, rotation, thumbnails, text search,
outline navigation, encrypted documents, session restore.

**Pages** — merge, split, extract, delete, reorder, rotate, crop, insert blank
or from another file, page numbers, Bates numbering.

**Annotations** — highlight, underline, strikethrough, squiggly, sticky notes
with replies, free text, rectangles, ellipses, lines, arrows, polygons,
freehand ink, stamps, measurement. Import and export as XFDF; flatten into the
page.

**Content editing** — edit existing text in place with font matching, add text
and images, edit and replace images, watermarks, backgrounds, headers and
footers, hyperlinks.

**Forms** — fill text, checkbox, radio and choice fields; create new fields;
set field properties and tab order; import and export form data; flatten.
Degrades gracefully on XFA documents rather than silently mis-rendering them.

**Signing and security** — certificate signing (PKCS#12, PAdES, `DocMDP`
certification levels), signature verification with per-signature status,
drawn/typed/image signature stamps, AES-256 encryption with user and owner
passwords and permission flags, true redaction that removes content rather than
covering it, pattern-based redaction with a confirm step, and document cleaning
(metadata, comments, attachments, bookmarks, form data, embedded files).

## Building from source

### Prerequisites

- **Node.js 22** or newer
- **Rust 1.80** or newer (`rustup`)
- Platform toolchain for Tauri 2 — see
  [tauri.app/start/prerequisites](https://tauri.app/start/prerequisites/).
  On macOS that is Xcode Command Line Tools; on Linux, `webkit2gtk` and
  friends; on Windows, the MSVC build tools and WebView2.

### Setup

```bash
git clone https://github.com/Lakshay989/VibePDF.git
cd VibePDF
npm install
npm run fetch-pdfium
```

`npm install` copies the PDF.js worker into place. `fetch-pdfium` downloads the
prebuilt PDFium binary the Rust side links against; it is the one step that
needs the network, and it only runs when you ask for it.

### Run

```bash
npm run dev
```

The first build compiles the Rust crate and takes several minutes. Later runs
are incremental, and the frontend hot-reloads.

### Package

```bash
npm run build
npm run tauri build
```

## Development

| Command | What it does |
|---|---|
| `npm run dev` | Tauri dev server with hot reload |
| `npm run check` | Type-check, lint, and `cargo clippy` — all warnings are errors |
| `npm run test` | Frontend tests (Vitest) |
| `npm run test:rust` | Rust tests, including the PDF engine |
| `npm run test:pdf` | Just the PDF-touching Rust tests |
| `npm run test:e2e` | End-to-end tests (WebdriverIO) |

`npm run check` and both test suites must be green before anything lands.

### Testing

Roughly 740 frontend tests across 125 files, and 80 Rust integration suites
that run against real PDFs in `tests/fixtures/`. Every fixture has a committed,
dependency-free generator script beside it, so the corpus can be rebuilt and
reviewed rather than taken on trust.

Two conventions are worth knowing if you contribute:

- **Tests for removal assert on the data, not the mechanism.** A redaction test
  greps the saved bytes for the secret rather than checking that a key was
  unset — the second passes against the bug where the object survives detached.
- **Cryptography is checked against an outside implementation.** Signatures are
  verified with `openssl cms -verify`, paired with a counter-test that requires
  a tampered document to be rejected. A signature computed over the wrong bytes
  passes every self-consistent test there is.

## Architecture

Two PDF engines, deliberately:

- **PDFium** (via `pdfium-render`) renders pages and reads structure. It is a
  widely deployed, BSD-licensed engine, so what it shows closely matches what
  most readers will show.
- **lopdf** performs structural surgery on the serialized bytes — the page
  tree, `/AcroForm`, content streams, encryption dictionaries — in between
  PDFium passes, never on a shared live handle.
- **PDF.js** draws the page in the browser view layer.

Writes go through a **per-document actor** that owns the document and serializes
every mutation through a mailbox, with an `Edit` command pattern where each
operation returns its own inverse for undo.

`src-tauri/src/security/` holds the code where mistakes are silent — a document
that looks protected and is not, or looks redacted and still carries the text.
Changes there require a human review pass on the diff regardless of whether the
tests pass.

Full detail in [`docs/04_ARCHITECTURE.md`](docs/04_ARCHITECTURE.md).

## Stack

| Layer | Choice |
|---|---|
| Shell | Tauri 2 (Rust + native WebView) |
| Frontend | React 18, TypeScript, Vite, Tailwind, zustand |
| Render | PDF.js |
| Read/write | PDFium + lopdf |
| Crypto | RustCrypto (`cms`, `x509-cert`, `rsa`, `pkcs12`) |
| OCR (planned) | Tesseract via `leptess`, bundled |
| AI (planned) | Local Ollama, off by default |

Reasoning for each, and the libraries that were rejected and why, is in
[`docs/03_TECH_STACK.md`](docs/03_TECH_STACK.md).

## Repository layout

```
├── src/                  React frontend
│   ├── app/              Dialogs, toolbars, panels
│   ├── view/             Viewer and per-page tool overlays
│   ├── tools/            Pure tool logic, framework-free and unit-tested
│   ├── ipc/              Typed wrappers over Tauri commands
│   └── state/            zustand stores
├── src-tauri/            Rust backend
│   ├── src/pdf/          Document actor, COS layer, page and content edits
│   ├── src/security/     Signing, encryption, redaction
│   ├── src/commands/     Tauri command surface
│   └── tests/            Integration tests against real PDFs
├── docs/                 Vision, spec, stack, architecture, roadmap
├── steps/                Per-phase work breakdown and verification sweeps
└── tests/fixtures/       Test PDFs, each with its generator
```

## Documentation

| Document | Contents |
|---|---|
| [`docs/01_VISION.md`](docs/01_VISION.md) | What this is and the constraints that define it |
| [`docs/02_PRODUCT_SPEC.md`](docs/02_PRODUCT_SPEC.md) | Every requirement, in EARS syntax — the source of truth |
| [`docs/03_TECH_STACK.md`](docs/03_TECH_STACK.md) | Library choices and rejections |
| [`docs/04_ARCHITECTURE.md`](docs/04_ARCHITECTURE.md) | Module layout, IPC, data flow |
| [`docs/05_ROADMAP.md`](docs/05_ROADMAP.md) | Phases and acceptance demos |
| [`docs/06_CONVENTIONS.md`](docs/06_CONVENTIONS.md) | Code style and error handling |

Every feature traces to a spec ID (`P6-SEC-010` and so on), and the code that
implements one carries a `SPEC:` comment naming it.

## Contributing

The roadmap is sequential by design — a phase is finished before the next one
starts. Contributions are most useful when they land inside the current phase
or fix something already built.

Before opening a pull request:

1. `npm run check`, `npm run test` and `npm run test:rust` are green.
2. Any change under `src-tauri/src/pdf/` comes with a deterministic test against
   a fixture PDF.
3. Every write path round-trips: the output is reopened and verified before it
   is returned.
4. New dependencies are justified in the commit message.

## License

Intended to be permissively licensed — MIT or Apache 2.0, per
[`docs/01_VISION.md`](docs/01_VISION.md), with every bundled dependency
license-compatible and no copyleft in the shipped binary.

**A `LICENSE` file has not been added yet.** Until one is, the code is
technically unlicensed and no usage rights are granted. This needs resolving
before any release.
