# 03 — Tech Stack

Every choice here has a reason. If Claude wants to deviate, the reason needs to be defeated, not the inertia of having chosen something different.

---

## Shell — Tauri 2

**Choice:** Tauri 2 over Electron.

**Why:**
- Bundle size: ~8 MB installer vs Electron's ~120 MB. For "the free editor you keep open all day," install size matters.
- Memory: 30–60% lower idle RAM in benchmarks. PDFs are already memory-hungry; the shell shouldn't be too.
- Security model: capability-based IPC, default-deny native access. Easier to audit than Electron's "everything is Node.js."
- Mobile path: Tauri 2 ships iOS+Android. Out of scope for v1 but a free option.
- Native Rust backend: heavy PDF work lives in a real systems language with PDFium bindings already mature.

**Trade-off accepted:** Slightly less consistent rendering across Linux distros (WebKitGTK quirks). Mitigated by writing the rendering layer against PDF.js, not against WebView CSS features.

**Trade-off rejected:** Slower build times than Electron. We don't care; CI is cheap and devs run incremental builds.

---

## Frontend — React 18 + TypeScript + Vite

**Why React:** Maturity, ecosystem, and the most existing examples for PDF.js integration. The team's familiarity is irrelevant (it's an AI building this), but the corpus Claude has seen in training matters a lot.

**Why TypeScript strict:** PDF objects have complex shapes (operators, dictionaries, streams). Untyped JavaScript will lose data silently. `strict: true`, no `any`, no `@ts-ignore` without a justifying comment.

**Why Vite over Next.js / Remix:** This is not a web app. There is no server. There is no routing. Vite gives us fast HMR, ESM imports, and the smallest viable build pipeline.

**Styling — Tailwind CSS:** Utility-first matches a tool UI where dozens of small controls share styling. No design system framework (no MUI, no Chakra) — we want absolute control over the visual language.

**Component primitives — Radix UI:** Unstyled, accessible primitives for dialogs, menus, tooltips, dropdowns. Pairs with Tailwind. Avoids reimplementing focus management and ARIA. Apache 2.0.

**State — Zustand:** Light, no boilerplate, scales fine for an editor. Each major feature (current doc, tool state, annotations, settings) gets its own store. No Redux.

**Persistence — Tauri's file system + IndexedDB:** Settings and recent files in app config dir. Per-document state (annotations being added) in IndexedDB so a crash recovers.

---

## PDF rendering — PDF.js (Mozilla)

**Why:** The most battle-tested PDF renderer in the world (it ships in Firefox, has been refined for over a decade). Apache 2.0 license. Excellent text-layer support for selection and search. Strong accessibility tree.

**How we use it:** Rendering only. We do NOT use PDF.js's experimental editor features (limited to 5 annotation types, save-back is fragile per the [Nutrient teardown of PDF.js editing](https://www.nutrient.io/blog/complete-guide-to-pdfjs/)).

**Integration:** Imported as ESM, runs in the WebView. Web Worker for parsing to keep the main thread responsive. Canvas rendering by default, SVG fallback for accessibility.

---

## PDF mutation — PDFium via `pdfium-render` (Rust)

**Why:** Google PDFium is the engine inside Chromium. BSD-licensed. C++ at its core, with mature Rust bindings (`pdfium-render`). Supports the full surface area we need: text editing, annotation, forms, signatures, redaction.

**Specifically why not the alternatives:**

| Library | License | Why we passed |
|---|---|---|
| **MuPDF** | AGPL (free) or commercial | AGPL is contagious; would force VibePDF to be AGPL. Not acceptable for our license posture. |
| **pdf-lib** (JS) | MIT | Pure JS, runs in browser. Great for simple operations. Cannot edit existing text reliably; doesn't expose PDFium's text-object granularity. Useful as a fallback for trivial ops; not the primary engine. |
| **PDFKit** (JS) | MIT | Generation only. Cannot edit existing PDFs. Out of scope. |
| **Apache PDFBox** (Java) | Apache 2.0 | Would force a JVM in the bundle. Not acceptable for install size. |
| **iText** (Java) | AGPL or commercial | Same license problem as MuPDF. |
| **PoDoFo** (C++) | LGPL | Workable but smaller community than PDFium, harder bindings story. |

**Bindings choice:** Between `pdfium-render` (ajrcarey) and `pdfium` (newinnovations), we choose `pdfium-render`:
- Larger user base, more examples in the wild
- Documents complete API surface (form fields, annotations, signatures)
- Active maintenance with frequent PDFium version bumps

The newer `pdfium` crate (with thread-safe init via `parking_lot::ReentrantMutex`) is GPL-licensed, which we reject for the same reason as MuPDF.

**Binary distribution:** We use [bblanchon/pdfium-binaries](https://github.com/bblanchon/pdfium-binaries) for prebuilt PDFium libraries (BSD/Apache 2.0). Bundled into the Tauri installer per platform.

**Thread safety:** PDFium is single-threaded per-document. We wrap each open document in an actor (an `mpsc` channel + dedicated thread) so the rest of the app can interact with it concurrently without holding locks.

---

## OCR — Tesseract via `leptess` (Rust)

**Why Tesseract:** 100+ languages, mature, Apache 2.0. The gold standard for offline OCR.

**Why `leptess` (Rust binding):** Stable Rust bindings, exposes the LSTM engine, supports getting word/line bounding boxes (needed for the invisible-text-layer alignment when producing searchable PDFs).

**Trained data:** We ship the English LSTM data (~15 MB) in the installer. Other languages are downloaded on demand from `tessdata_fast` (BSD license) and cached in the app data dir.

**Pre-processing:** Before passing images to Tesseract we run a small Rust pipeline: deskew (via `imageproc`), denoise (median filter), and bicubic upscale if input is < 300 DPI. These preprocessing choices double OCR accuracy in our reference fixtures.

**Alternative considered:** Tesseract.js (WebAssembly). Rejected because native Tesseract via leptess is ~3-5× faster on multi-page docs and avoids holding the WebView main thread.

---

## Crypto & signing — `rsa`, `x509-cert`, `cms`

**Why these crates:** RustCrypto's pure-Rust ecosystem. Apache 2.0 / MIT. No OpenSSL dependency to wrestle with at install time.

- `rsa` — RSA operations (signing, encryption)
- `x509-cert` — Certificate parsing and chain validation
- `cms` — Cryptographic Message Syntax (the structure inside PKCS#7 signatures)
- `aes` + `aes-gcm` — Document encryption (256-bit AES per PDF 2.0)

**PDF signature flow:**
1. Compute a SHA-256 hash of the byte range being signed
2. Build a PKCS#7 signed-data structure
3. Embed in the PDF Signature dictionary
4. PAdES-compliant by default (LTV-ready)

We will NOT implement custom crypto. Every primitive comes from RustCrypto.

---

## Local AI — Ollama HTTP + ONNX Runtime

**Architecture:** Two backends, used for different tasks.

**Ollama** (when available locally at `http://localhost:11434`) — for generative tasks:
- Summarization
- Q&A
- Translation

Default models suggested: `llama3.1:8b-instruct-q4` for general, `phi4:14b` if the user has the RAM.

**ONNX Runtime via `ort` crate** — for deterministic, fast, small-model tasks:
- PII NER (a fine-tuned `distilbert-base-multilingual-cased` or a custom ONNX model we ship)
- Embedding generation for semantic search (`bge-small-en-v1.5` ONNX, ~30 MB)

**Why both:** Generative LLMs are heavy and the user may already have Ollama. NER and embeddings need to be fast, deterministic, and bundled — ONNX is the right tool. Mixing them is fine: each has clear domains.

**Hard rule:** the AI subsystem must be optional. The Phase 8 commit must add no required dependencies to earlier phases. AI features hide entirely if no backend is configured.

---

## Testing

| Layer | Tool | Why |
|---|---|---|
| Rust unit/integration | `cargo test` + `insta` for snapshots | Standard. Insta for PDF binary snapshots. |
| TS unit | `vitest` | Vite-native, fast. |
| TS component | `vitest` + `@testing-library/react` | Standard. |
| E2E | `tauri-driver` + Playwright | Drives the actual built app. |
| PDF regression | Custom harness: open fixture → apply op → compare bytes/visual against golden | The most important test layer. |
| Visual regression | `pixelmatch` + golden PNG | For rendering correctness. |

Every fixture in `tests/fixtures/` has provenance documented (where the PDF came from, what makes it tricky).

---

## Build & release

| Concern | Tool | Why |
|---|---|---|
| Bundler | Tauri's built-in (`tauri build`) | Produces platform installers (.msi, .dmg, .deb, .AppImage). |
| Code signing — Windows | `signtool` via Tauri config | Required to avoid SmartScreen warnings. |
| Code signing — macOS | `codesign` + notarization | Required to avoid Gatekeeper warnings. |
| Code signing — Linux | None | Linux users self-verify. |
| Updates | `tauri-plugin-updater` | Signed update manifests, no telemetry. |
| CI | GitHub Actions, matrix build | Free for OSS. |

---

## What we will not introduce without an architectural review

These dependencies are off-limits as quick additions:

- Any GPL or AGPL library
- Any cloud SDK (AWS, Azure, GCP)
- Any analytics or error-tracking SDK (Sentry, PostHog, etc.)
- Any WebAssembly PDF library that competes with PDFium (no double engines)
- Any Java/JVM runtime
- Any "free for non-commercial use" library
- Any library not on crates.io or npm (no `git` dependencies in production)

---

## Versions locked at project start

These are the floors. Update freely; just keep the lock file consistent.

```
# Tauri
tauri = "2.x"
tauri-build = "2.x"

# Rust PDF
pdfium-render = "0.8.x"

# OCR
leptess = "0.14.x"

# Crypto
rsa = "0.9.x"
x509-cert = "0.2.x"
cms = "0.2.x"
aes = "0.8.x"
aes-gcm = "0.10.x"

# AI
ort = "2.x"
reqwest = "0.12.x"  # for Ollama HTTP

# Frontend
react = "18.x"
typescript = "5.x"
vite = "5.x"
tailwindcss = "3.x"
@radix-ui/react-* = latest
zustand = "4.x"
pdfjs-dist = "4.x"
```

When Claude runs `cargo add` or `npm install`, it must look up the latest patch within these majors. It should NOT guess versions.
