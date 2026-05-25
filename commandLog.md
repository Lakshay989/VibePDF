# commandLog.md — Every command that materially shaped this repo

> Append after each step. Goal: someone reading this in 2027 can
> replay the project from scratch by running these in order.
>
> **What goes in:** anything that mutated the repo, the toolchain,
> or your machine state. Setup commands, installs, generators,
> scripts, the test invocations we use as gates.
>
> **What doesn't:** transient debugging (`cat`, `grep`, `ls`),
> exploratory `git log`, anything the formatter / IDE runs for you.

Format per entry:

```
$ <literal command>
# what it does · why we needed it
```

---

## 0. Tooling prerequisites (your machine)

The repo assumes:

```bash
$ curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# Installs rustup, cargo, rustc · needed for src-tauri/ build + tests
$ source "$HOME/.cargo/env"
# Make cargo visible in the current shell · only needed in the install session

$ nvm install 22 && nvm use 22
# Node 20+ · required by Vite 8 / Vitest 2 (rolldown imports node:util.styleText)

$ xcode-select -p
# Confirms full Xcode is installed · required for Tauri's macOS native build
```

Outcome: `rustc --version` works, `node --version` reports ≥ 20, `xcode-select -p` returns a path.

---

## 1. Bootstrap (commit `c7a54f5`)

### Repo reorganization

```bash
$ mkdir -p docs .claude/commands
# Create target dirs · the kit shipped docs flat at the root

$ git mv 01_VISION.md 02_PRODUCT_SPEC.md 03_TECH_STACK.md 04_ARCHITECTURE.md \
         05_ROADMAP.md 06_CONVENTIONS.md 07_AI_FEATURES.md docs/
# Move spec docs under docs/ · CLAUDE.md and 04_ARCHITECTURE.md assume this layout

$ git mv plan.md review.md ship.md test-pdf.md .claude/commands/
$ git mv settings.example.json .claude/
# Move slash-command files into .claude/commands/ · matches the kit's documented layout
```

### Frontend dependencies

```bash
$ npm install
# Installs 351 packages: react@19, vite@8, tailwindcss@4, pdfjs-dist@5,
# zustand@5, @tauri-apps/api@2, @tauri-apps/plugin-fs, @tauri-apps/plugin-dialog,
# typescript@6, eslint@9, vitest@2, ... · everything declared in package.json

$ npm install -D @types/node
# Node runtime types · needed by src/view/__tests__/render-page.test.ts (uses node:fs/promises)
```

### Test fixture generation

```bash
$ python3 tests/fixtures/basic/generate-hello.py
# Writes tests/fixtures/basic/hello.pdf (596 bytes, valid PDF 1.4, 1 page)
# · used by both the Rust integration test and vitest. No external deps; pure stdlib.

$ file tests/fixtures/basic/hello.pdf
# Verifies → "PDF document, version 1.4, 1 pages"
```

### PDFium binary fetch (your machine only; not committed)

```bash
$ chmod +x scripts/fetch-pdfium.sh
$ npm run fetch-pdfium
# Pulls a prebuilt PDFium dylib for the current platform from
# bblanchon/pdfium-binaries into src-tauri/resources/pdfium/.
# REQUIREMENT for `npm run dev` and for `cargo test`.
```

### Verification gates (run on every step)

```bash
$ npx tsc --noEmit
# TypeScript type-checks the whole src/ tree without emitting JS.
# Gate: must be clean (no output) before a commit.

$ npx eslint src --max-warnings=0
# Lint pass. Zero warnings; warnings fail the gate.

$ npx vitest run
# Runs every *.test.ts under src/. Blocked on Node 18 (rolldown);
# unblocks on Node 20+.

$ cd src-tauri && cargo check
# Type-checks the Rust crate without building binaries.

$ cd src-tauri && cargo test
# Runs Rust unit + integration tests. Requires PDFium binary present
# (see fetch-pdfium above).
```

### Run the app

```bash
$ npm run dev
# Tauri dev server with hot reload. Frontend at http://localhost:1420
# served inside a native webview window.
```

---

## 2. Phase / step infrastructure (commits `9a54085`, `3ecd9d1`)

```bash
$ mkdir -p steps
$ git mv steps.md steps/P1.md
# Split the single-file Phase 1 plan into a steps/P<n>.md tree;
# steps.md at root becomes the index.

# Back-tracking patterns (read-only, but worth knowing):
$ git log --grep="P1.C2"
# Find the commit that shipped step P1.C2.
$ git log --grep="P1-VIEW-006"
# Find every commit referencing spec line P1-VIEW-006.
```

---

## 3. Per-step commands

### P1.A1 — Drag-and-drop file open (commit `9313aaf`)

```bash
$ npx tsc --noEmit                                       # pass
$ npx eslint src --max-warnings=0                        # pass
$ npx vitest run src/app/__tests__/drag-drop.test.ts     # blocked on Node 18
```

No new dependencies.

### P1.C1 — Virtual-scrolling page list (commit `d10c601`)

```bash
$ npx tsc --noEmit                                       # pass
$ npx eslint src --max-warnings=0                        # pass (after adding HTMLDivElement, IntersectionObserver to eslint globals)
$ npx vitest run src/view/__tests__/page-cache.test.ts   # blocked on Node 18
```

No new dependencies.

### P1.C3 — Keyboard navigation (commit `07f0b3a`)

```bash
$ npx tsc --noEmit                                       # pass
$ npx eslint src --max-warnings=0                        # pass (added Element to globals)
$ npx vitest run src/view/__tests__/keyboard-nav.test.ts # blocked on Node 18
```

No new dependencies.

### P1.C2 — Zoom + fit modes + per-document persistence (commit `16e31bc`)

```bash
$ npm install -D fake-indexeddb
# In-memory IndexedDB shim for the vitest IDB round-trip test.
# Imported at the top of the test as `import "fake-indexeddb/auto"`.

$ npx tsc --noEmit                                                          # pass
$ npx eslint src --max-warnings=0                                           # pass (added ResizeObserver, TextEncoder, crypto, indexedDB, IDBDatabase)
$ npx vitest run src/state/__tests__/view-persistence.test.ts               # blocked on Node 18
```

### P1.C5 — Dark-mode page invert (commit `184f0e5`)

```bash
$ npx tsc --noEmit                                            # pass
$ npx eslint src --max-warnings=0                             # pass (added MutationObserver, Uint8ClampedArray to globals)
$ npx vitest run src/view/__tests__/dark-invert.test.ts       # blocked on Node 18
```

No new dependencies.

### P1.D2 — Outline sidebar (this commit)

```bash
$ npx tsc --noEmit                                            # pass
$ npx eslint src --max-warnings=0                             # pass
$ npx vitest run src/panels/__tests__/outline-tree.test.ts    # blocked on Node 18
```

No new dependencies.

---

## How this file evolves

Every step commit appends a `### P<n>.<id> — <name> (commit <sha>)`
section. Include:

- Any `npm install` / `cargo add` / `pip install` that step needed.
- Any script invocation that mutated the repo (fixture generation, codegen).
- The three verification commands (`tsc`, `eslint`, `vitest` and/or `cargo test`).
- One-line "what / why" comments — terse, but enough to know whether
  you still need to run it on a fresh clone.

If a command **doesn't** mutate state and isn't a verification gate
(e.g. you ran `git log` to find a SHA, or `grep` to count step
headings), leave it out.
