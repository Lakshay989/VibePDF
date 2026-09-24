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

**Why:** Google PDFium is the engine inside a major browser. BSD-licensed. C++ at its core, with mature Rust bindings (`pdfium-render`). Supports the full surface area we need: text editing, annotation, forms, signatures, redaction.

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

### Structural edits — `lopdf` (COS / object model)

**Why a second library:** `pdfium-render`'s high-level API is **read-only** for the document `/Outlines` (bookmarks) and the `/AcroForm` (interactive form fields), and it exposes no way to rewrite the page tree or indirect references. That blocks four spec clauses: **P2-PAGE-002** (reorder must "update all internal references"), **P2-PAGE-003** (delete ref cleanup), **P2-PAGE-005** (insert must "preserve form fields"), and **P2-PAGE-008** (merge must "preserve bookmarks, form fields" + uniquify colliding field names). `lopdf` (MIT, pure Rust, on crates.io) is a read/write model of the PDF object graph (COS = the Carousel Object System) — exactly the dictionary-level surgery PDFium won't do.

**This is *not* a "double engine"** (the thing line "no WebAssembly PDF library that competes with PDFium" guards against). `lopdf` does **not** render and does **not** edit content streams — zero overlap with PDFium's job. It does only the object/dictionary rewrites PDFium's API can't reach. The two are **complementary**, and this subsection is the architectural review that the "off-limits without review" list requires.

**Integration model — byte handoff, never a shared handle.** PDFium and `lopdf` never hold the same live document at once. A structural edit is a pass over a **byte buffer** that sits *between* PDFium passes: PDFium produces bytes (`save_to_bytes`) → `lopdf` loads them, rewrites the object graph, re-serializes → PDFium reloads / the bytes are saved. `lopdf` is pure Rust, so it needs no `PDFIUM_LOCK` and can't race the actors. **Every `lopdf` output is round-trip-verified by reopening it in PDFium** before it's persisted (`verify_pdf_reopens`); the `cos.rs` spike tests assert this on real fixtures. See `docs/04` "Structural edits via lopdf".

**Alternatives passed:** the `pdf` crate (read-focused, weaker write story); `pdf-writer` (generation only, can't load+edit existing files); hand-rolling a COS parser/serializer (re-implements xref/object-streams/refs that `lopdf` already does, with far less testing and all the round-trip risk on us — worse than adopting a proven MIT lib).

**Features:** `default-features = false` — we need only the core object model + the `nom` parser, not date parsing (`chrono`/`jiff`/`time`) or `rayon`. The transitive tree (RustCrypto `aes`/`cbc`/`ecb`/`md-5` for `/Encrypt`, `flate2` for `FlateDecode`, `nom`, `encoding_rs`, `indexmap`) is all permissive (MIT/Apache/BSD) — audited, no GPL/AGPL.

**Known limits:** encrypted-PDF *structural* edits and exotic object-stream layouts are not yet exercised beyond our fixtures; revisit when a real file needs them.

### Font embedding + subsetting — PDFium for encoding, `subsetter`/`ttf-parser` for size (P4.HF5 + HF6)

Rendering text outside the built-in base-14 fonts' WinAnsi range (CJK, Cyrillic, Greek, …) requires an *embedded* font. **The encoding side takes no font parser:** PDFium (already linked) contains a full font engine, so `pdf/font_embed.rs` calls `PdfFonts::load_true_type_from_bytes` + `create_text_object` and PDFium writes the `/Type0` + `/CIDFontType2` + `/ToUnicode` + `/FontFile2` itself. `font_resolver.rs`'s "no font parser for *name matching*" stance is unchanged (it still only matches names).

**The size side, however, does need a parser (P4.HF6).** PDFium embeds the *whole* face — it does not subset — so a Cyrillic footer using Arial Unicode came out **15 MB**. PDFium's native subset flag (`FPDF_SUBSET_NEW_FONTS`) is unreachable through `pdfium-render` 0.9.1 (the document handle + file-writer are `pub(crate)`, and `save_to_writer` hardcodes `flags = 0`). So we subset the face *ourselves* before handing it to PDFium, in `font_embed::subset_font`:

- **`subsetter` (0.1, MIT/Apache, zero-dependency)** — the Typst team's PDF font subsetter. `Profile::pdf(&glyph_ids)` keeps only the used glyphs *and preserves original glyph-ids + the `cmap`*, so PDFium's Unicode→GID lookup still resolves on the subset (verified: the same footer is now **60 KB**, ~256× smaller).
- **`ttf-parser` (0.25, MIT/Apache, zero-dependency)** — RazrFalcon's read-only parser, used only to map the runs' codepoints → glyph-ids for the subset set.

**Why this is consistent with the "no double engine / minimal deps" rules:** both crates are permissive, zero-transitive-dependency, and read/produce font *bytes* — they don't render or edit PDFs (no overlap with PDFium or lopdf). They're the smallest tools that do exactly one thing PDFium won't. `subset_font` degrades gracefully — an unparseable or un-subsettable face embeds whole (bloated but correct), never a hard failure. *(The heavier `subsetter` 0.2.x / `fontations` stack and `allsorts` were passed over for tree size.)*

---

## OCR — Tesseract via `tesseract-rs` (Rust)

**Why Tesseract:** 100+ languages, mature, Apache 2.0. The gold standard for offline OCR, and what `P7-OCR-001` names.

**Why `tesseract-rs` and not `leptess` (decided 2026-09-21, P7.A1):** `leptess` was the original pick here and has had no release since **2023-02-21**; `steps/P7.md` carried a standing warning to re-evaluate before Track A started. Both bind the same C API, so the choice came down to how Tesseract reaches a user's machine:

| Option | Verdict |
|---|---|
| **`tesseract-rs` with `build-tesseract`** (chosen) | Compiles Tesseract 5.5.2 + Leptonica 1.87.0 from source and links them statically. Nothing to install, one story on all three platforms, MIT crate, actively released (0.4.0, 2026-07-31). |
| `leptess` / `tesseract` against system libraries | Fast builds, but every contributor installs `libtesseract`/`libleptonica`, and a release would have to copy those dylibs into the bundle by hand per platform. |
| Bundling the `tesseract` command-line program | Simplest linking and crash isolation, but we would have to produce and sign that binary for three platforms ourselves. Kept as the fallback if static linking ever becomes the tail that wags the dog. |

**What the crate's build script does, and what we do about it:** it downloads both source archives over the network with no integrity check, and separately insists on two `*.traineddata` files, failing the build when the network is slow. `scripts/fetch-tesseract-src.sh` pre-places all four, verified against pinned SHA-256s, in the cache directory the build script reuses — so a build performs no download of its own. This is the same posture as `scripts/fetch-pdfium.sh`: the bytes a build compiles are pinned, and substitution is detectable.

**Trained data (P7.A3):** all twelve languages `P7-OCR-002` names — English, Spanish, French, German, Chinese (both scripts), Japanese, Korean, Arabic, Hindi, Portuguese, Russian — ship in the build (`tessdata_fast`, ~25 MB total, Apache-2.0), fetched by `scripts/fetch-tessdata.sh` with a pinned checksum each. `ocr::tessdata` resolves them at runtime and refuses to fall back to a system-wide Tesseract, so a developer machine cannot pass a test a user's machine would fail.

A thirteenth language arrives either **from a file** the user has, or **by download** — the application's only network call, off until switched on, never in the background, and always for a language someone just asked for (`ocr::languages`). Both paths refuse anything Tesseract cannot load, because a language pack that fails later, inside a document, is worse than one that was never installed. Downloads use `ureq` (MIT/Apache-2.0, rustls with the platform trust store); its bundled CA list is CDLA-Permissive-2.0, reviewed and recorded in `scripts/generate-notices.mjs`.

**Writing non-Latin text (P7.A3):** eight of the twelve languages are outside WinAnsi, so the invisible text layer embeds a covering system face through the same CID path the header/footer writer uses. Without it those languages recognise perfectly and write nothing — the failure mode a "supports twelve languages" claim hides.

**Pre-processing — hand-written, not `imageproc`:** deskew, denoise and upscale are ~40 lines each over an 8-bit grayscale buffer (`ocr::preprocess`), so they carry no new dependency, in the same spirit as using `png` directly instead of `image`.

Two findings worth keeping from building it:

- **A plain 3×3 median filter is the wrong "denoise" for text.** Measured 2026-09-21 on 9 pt text rendered at 150 DPI: it cost 3 of 10 words (`brown` → `broval`), because a stroke is 2–3 pixels wide at that size and a median rounds it away. Replaced with a despeckle that only rewrites isolated outliers; it touches 11–18 pixels of a 2-megapixel page and the same page then reads perfectly.
- **Fixtures must use body-text sizes.** At 28 pt every pipeline scored 10/10, including the harmful one. `tests/fixtures/basic/scan.pdf` now carries 9 pt lines for exactly this reason.

**Alternative considered:** Tesseract.js (WebAssembly). Rejected because native Tesseract is ~3-5× faster on multi-page docs and avoids holding the WebView main thread.

---

## Image encoders — `png` direct, `image` for the rest (P7.B2)

P7-OCR-005 names four output formats — PNG, JPG, TIFF, WebP — and until P7.B2 the tree had an encoder for exactly one of them (`png = "0.17"`, added in P1.B3 for the render IPC reply).

**Why `image` 0.25:** it was **already in `Cargo.lock`** as a transitive dependency of `pdfium-render`, so taking a direct edge on it with `default-features = false, features = ["jpeg", "tiff", "webp"]` adds three codecs and no new top-level supply chain. Six crates came with them — `tiff`, `image-webp`, `fax`, `half`, `crunchy`, `quick-error` — all permissive, and the licence gate (`npm run licenses`) is the check. The alternative, hand-writing a TIFF and a WebP encoder, is strictly worse than using the image-rs maintainers' own.

**PNG stays on the `png` crate.** It is the hot path — every page render on the view layer goes through `render.rs::encode_png` — and it is the thinner call. `export_image.rs` reuses that same function rather than routing PNG through `image`, so the two paths cannot drift.

Two limits worth stating rather than discovering:

- **WebP is lossless only.** `image` 0.25 ships the VP8L encoder; lossy WebP needs the C `libwebp`, which would cost us the "compiles anywhere with no system library" property that `tesseract-rs` already strains. A lossless WebP page is smaller than the PNG and larger than a lossy WebP would be.
- **JPEG is three-channel, so the RGBA render is composited down.** Measured 2026-09-23: PDFium renders fully opaque (0 of 484,704 pixels of `hello.pdf` at 72 DPI had `a != 255`), so compositing over *white* rather than simply dropping the alpha channel is insurance, not a live fix — but it is the difference between a correct page and a black one if a future PDFium, or a caller that sets a clear colour, ever hands us real transparency.

---

## Compression — `lopdf` + the `image` encoders (P7.C2a)

P7-OCR-010 names three targets. Two are implemented in `pdf/compress.rs`; the third, font subsetting, is deliberately deferred to its own step (decided 2026-09-23) — doing it properly means walking every content stream to learn which glyphs are used, and it saves nothing on the scanned documents the feature exists for.

**Where the bytes actually are**, measured 2026-09-23 on a 20-page 300 DPI grayscale scan with sensor noise (81.6 MB, `tests/fixtures/basic/generate-noisy-scan.py`):

| | 300 DPI | 200 DPI | 150 DPI |
|---|---|---|---|
| JPEG q85 | 20.4% | 9.1% | 5.2% |
| JPEG q60 | 5.6% | 2.5% | 1.5% |

Stream deflation is the opposite shape: nothing on a scan (every image stream is already Flate), and 92.8% on `unicode-text.pdf`, which stores its embedded font program uncompressed.

**Why `lopdf` and not `PDFium` for the image pass.** `PdfPageImageObject::set_image` calls `FPDFImageObj_SetBitmap`, which stores a **raw BGRA bitmap** — it would inflate every image fourfold and turn a `DeviceGray` scan into four channels. PDFium's only compressing path, `FPDFImageObj_LoadJpegFileInline`, is reachable through `pdfium-render` solely when *creating* an object, not when replacing one.

**Triangle, not Lanczos3, for the downsample.** Measured on a 2550×3300 scan page reduced to 200 DPI: Triangle took 3.3 s against Lanczos3's 6.7 s *and* produced a 21% smaller JPEG (241 KB against 305 KB). Lanczos3 rings, which preserves exactly the sensor noise the JPEG then has to spend bits on; a bilinear kernel low-passes it away. Faster and smaller on the same input is not a trade-off.

**Two rules keep it safe to offer.** Nothing is replaced unless the replacement is smaller — per image *and* for the file as a whole — and nothing is touched whose colour cannot survive the trip. The colour space is read only to learn the channel count; the `/ColorSpace` entry itself is never rewritten, so ICC-managed and calibrated images keep their profile across the re-encode. `/Indexed`, `/Separation` and `/DeviceN` are refused outright, because a sample there is an index or an ink quantity rather than a colour.

Compression writes a **new file** rather than becoming an undoable edit: the image pass is lossy, and an edit would let the next Ctrl-S overwrite the user's original with the degraded version.

---

## Word export — hand-written OOXML (P7.B1a)

A `.docx` is a ZIP of five small XML parts. `pdf/ooxml.rs` writes that container directly: local file headers, a central directory, an end-of-central-directory record, and nothing else — no Zip64 (the parts are kilobytes), no encryption, no streaming.

**Why not the `zip` crate.** It is in `Cargo.lock`, but only as a build dependency of the Tauri bundler; `cargo tree -e normal -i zip` finds nothing, so taking it would have pulled `zip`, `zopfli`, `typed-path` and friends into the shipped binary. `flate2` (via lopdf) and `crc32fast` (via flate2) were already reachable, and they are the whole of what a ZIP entry needs — so the two direct edges added here cost **zero new crates**, which the licence report confirms (410 before and after).

The risk of a hand-written container is that a subtle error produces a file Word silently refuses, so it is checked three ways: our own reader for content, `unzip -t` in the test suite for the format, and Word itself in the acceptance pass. That layering earned its keep immediately — a deliberately corrupted central-directory offset was invisible to our reader (which walks local headers) and caught only by `unzip -t`.

**Headings are relative, never absolute.** A PDF has no heading flag; it has type that is larger than its neighbours. The body size is whichever size the most *characters* are set in — counting runs would let a three-word title outvote a paragraph — and a line materially larger than that is a heading, ranked by size. A document set in one size gets no headings at all.

Measured 2026-09-23: the first width rule (a heading must be under 60% of its column) rejected a plain 20 pt title that ran to 63%. The rule now applies only below 1.5× the body size, where length is genuinely the tie-breaker between a heading and a lead paragraph.

**Tables are detected from ruling lines only** (P7.B1b). There are two ways to guess that some text is a table: ruling lines are *evidence* — the document drew a grid — while alignment is a *guess* that fires on two-column layouts, contents lists and forms. A wrong table is worse for a reader than no table, because it locks prose into cells they then have to unpick, so an unruled table comes out as paragraphs.

Four guards stand between a page's rules and a table: at least two horizontal and two vertical grid lines, lines that actually bound a region, at least 2×2 cells, and text inside. A section rule under a masthead has no verticals; a box round a pull-quote has one cell; a bordered sidebar has one column. The fixture carries a decoy rule so that failure stays visible.

One measured trap: **`PDFium` reports a path segment's point before the object matrix is applied, while its bounding box is after.** Measured 2026-09-23 — a line written `10 10 m 100 10 l` under `2 0 0 2 20 40 cm` reports points `(10,10)-(100,10)` and bounds `(38.5,58.5)-(221.5,61.5)`. A detector that trusts the raw points mislocates every rule in a transformed document, and most real documents are transformed, so one fixture page is drawn under a `cm` purely to keep that regression caught.

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
| E2E | `tauri-driver` + WebdriverIO | Drives the actual built app. (Originally listed as Playwright, but Playwright speaks CDP/its own protocol and can't drive a Tauri webview; `tauri-driver` implements W3C WebDriver, which WebdriverIO/Selenium speak. WebdriverIO is the officially documented Tauri 2 E2E stack. Linux/Windows only — `tauri-driver` has no macOS support.) |
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
  - *Reviewed & approved (this list's required review): `lopdf` is **not** a
    competing engine — it's a COS/object-model layer (no render, no content-stream
    edits) that does only the dictionary rewrites PDFium's API can't. See "PDF
    mutation → Structural edits — `lopdf`".*
- Any Java/JVM runtime
- Any "free for non-commercial use" library
- Any library not on crates.io or npm (no `git` dependencies in production)

---

## Versions locked at project start

These are the floors as of the **bootstrap commit (2026-05-22)**. They reflect the live majors on crates.io / npm at that date — the original draft floors (Vite 5, React 18, Tailwind 3, pdfjs-dist 4, Zustand 4, pdfium-render 0.8) were several majors stale. Update freely going forward; just keep the lock file consistent.

```
# Tauri
tauri        = "2.11"
tauri-build  = "2.11"

# Rust PDF
pdfium-render = "0.9"        # current stable as of May 2026
lopdf        = "0.36"        # COS/object-model layer (structural edits PDFium
                             # can't do); default-features off. 0.41 exists but
                             # needs a newer Rust than our 1.80 floor.

# OCR
leptess = "0.14"             # WARNING: last release 2023-02-21. Re-evaluate before Phase 7;
                             # alternatives: tesseract-rs, custom bindgen wrapper.

# Crypto
rsa       = "0.9"
x509-cert = "0.2"
cms       = "0.2"
aes       = "0.8"
aes-gcm   = "0.10"

# AI
ort     = "2.0.0-rc"          # only RC releases exist as of May 2026; stable 2.x not yet out
reqwest = "0.12"              # for Ollama HTTP

# Frontend (current majors as of 2026-05-22)
react        = "19.2"
typescript   = "6.0"
vite         = "8.0"
tailwindcss  = "4.3"          # v4 is a rewrite; uses the @tailwindcss/vite plugin
@radix-ui/react-* = latest
zustand      = "5.0"
pdfjs-dist   = "5.7"          # v5 changes worker loader API; see src/view/pdfjs-worker.ts
```

When Claude runs `cargo add` or `npm install`, it must look up the latest patch within these majors. It should NOT guess versions.
