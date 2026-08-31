# VibePDF — Project Context

> This file is loaded at the start of every Claude Code session. Keep it short.
> Detail lives in `docs/`. Read those on demand.

## What we are building

VibePDF is an **offline, free, open-source PDF editor** for Windows / macOS / Linux. The goal is to match the paid editors on its core editing surface, with no paywall, no account, and no telemetry. See `docs/01_VISION.md` for the full pitch.

## Read these in order on first session

1. `docs/01_VISION.md` — what and why (2 min read)
2. `docs/02_PRODUCT_SPEC.md` — feature spec in EARS syntax (the source of truth)
3. `docs/03_TECH_STACK.md` — chosen libraries, with reasoning
4. `docs/04_ARCHITECTURE.md` — module layout, IPC, data flow
5. `docs/05_ROADMAP.md` — phased plan; we work one phase at a time
6. `docs/06_CONVENTIONS.md` — code style, naming, error handling
7. `docs/07_AI_FEATURES.md` — only when working on AI/ML features

Stop reading once a doc is irrelevant to the current task.

## Stack at a glance

- **Shell:** Tauri 2 (Rust + native WebView), not Electron
- **Frontend:** React 18 + TypeScript, Vite, Tailwind, zustand for state
- **PDF render:** PDF.js (Mozilla) on the frontend for the view layer
- **PDF mutate:** `pdfium-render` (Rust binding to Google PDFium) for all writes
- **OCR:** Tesseract via `leptess` (Rust) — bundled, no network
- **Text editing:** Two-phase redact+reflow using PDFium primitives
- **AI (optional):** local Ollama integration, off by default

Full reasoning lives in `docs/03_TECH_STACK.md`. Do not introduce additional PDF libraries without updating that doc.

## Working agreements

- **Plan before you code.** For anything that touches >1 file or >50 lines, enter plan mode (`Shift+Tab` twice), draft the change in EARS syntax against `docs/02_PRODUCT_SPEC.md`, and wait for human approval. Trivial mechanical edits skip this.
- **One phase at a time.** The roadmap is sequential by design. Do not start phase N+1 features while phase N still has open acceptance criteria.
- **Tests are not optional for the PDF engine.** Any change to `src-tauri/src/pdf/` must come with a deterministic test against a sample PDF in `tests/fixtures/`.
- **Offline-first is a hard constraint.** Any code path that needs the network must be gated behind an explicit user-enabled setting and must degrade gracefully when offline.
- **No silent breakage of existing PDFs.** Every write operation must round-trip the file through PDFium and verify the output opens cleanly before returning.

## Code style — the short version

- TypeScript: strict mode, no `any`, prefer `unknown` + narrowing
- Rust: `#![deny(warnings)]` in CI, `clippy::pedantic` enabled, `?` for errors, `anyhow` at boundaries and `thiserror` for typed errors inside modules
- File layout follows `docs/04_ARCHITECTURE.md` exactly — do not add new top-level modules without updating that doc first
- Naming: `camelCase` in TS, `snake_case` in Rust, `kebab-case` for files and routes
- Comments explain *why*, never *what*

Full conventions live in `docs/06_CONVENTIONS.md`.

## Workflow rules

- **Format/lint runs as a hook**, not in CLAUDE.md. If you edit a file, the formatter has already run by the time you see the diff. Don't reformat manually.
- **Single-file changes:** make them. Don't ask.
- **Multi-file changes:** plan first.
- **Architecture changes:** stop and write to `docs/04_ARCHITECTURE.md` first, then implement.
- **New dependencies:** require justification in the PR/commit message. Vendor lock-in is worse than implementation work.

## What "done" means

A feature is done when:
1. The behavior matches its EARS-syntax spec in `docs/02_PRODUCT_SPEC.md`
2. There is at least one passing test in `tests/` that exercises the new code path on a real PDF
3. `npm run check` (typecheck + lint + cargo clippy) **and** the test suites (`npm run test` + `npm run test:rust`) are green
4. The change has been demoed against the phase's acceptance PDFs in `tests/fixtures/acceptance/`

## Things Claude should never do without asking

- Add a new top-level dependency
- Change the chosen PDF library (PDFium / PDF.js)
- Touch `src-tauri/src/security/` (crypto/signing code)
- Write code that sends data over the network
- Modify `docs/02_PRODUCT_SPEC.md` — that is the human's responsibility unless told otherwise
- Delete or rewrite tests to make them pass

## Things Claude should do without asking

- Run formatters, linters, type-checkers
- Add new tests
- Refactor inside a single module to make code clearer
- Update internal docs (anything in `docs/` other than `02_PRODUCT_SPEC.md`)
- Suggest improvements at the end of a task

## Commands quick reference

| Command | What it does |
|---|---|
| `npm run dev` | Tauri dev server with hot reload |
| `npm run check` | Typecheck + lint + `cargo clippy` (no tests) |
| `npm run test` | Frontend tests (Vitest) |
| `npm run test:rust` | All Rust tests (`cargo test`, PDFium on the loader path) |
| `npm run test:pdf` | The PDF-touching Rust tests (render + actor + encrypted) |
| `/plan <feature>` | Draft spec + plan for a feature, then wait |
| `/ship <feature>` | Implement the most recent plan |
| `/review` | Self-review pass on the working tree |
| `/test-pdf <path>` | Run the standard regression set against one file |
