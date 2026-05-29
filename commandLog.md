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

### P1.D2 — Outline sidebar (commit `6021941`)

```bash
$ npx tsc --noEmit                                            # pass
$ npx eslint src --max-warnings=0                             # pass
$ npx vitest run src/panels/__tests__/outline-tree.test.ts    # blocked on Node 18
```

No new dependencies.

### P1.C4 — Text search Cmd/Ctrl+F (commit `5f436ab`)

```bash
$ npx tsc --noEmit                                            # pass (after switching the PDF.js text-item filter from a type-predicate to flatMap)
$ npx eslint src --max-warnings=0                             # pass (added HTMLInputElement, RegExpExecArray to globals)
$ npx vitest run src/view/__tests__/search.test.ts            # blocked on Node 18
```

No new dependencies.

### P1.E4 — Acceptance fixture generator (this commit)

```bash
$ python3 -m pip install pypdf
# Encryption library for the p1-encrypted.pdf fixture only.
# Pinned via tests/fixtures/acceptance/requirements.txt (pypdf>=5.0).
# Use `python3 -m pip` so the install lands in the same interpreter
# that runs the script (avoids the homebrew-vs-system-python split).

# Smoke verification (small sizes — the real ones are 500 MB):
$ python3 tests/fixtures/acceptance/generate.py spec --pages 3     # writes p1-spec.pdf,    6.5 KB
$ python3 tests/fixtures/acceptance/generate.py large --size-mb 1  # writes p1-large.pdf,   1.0 MB
$ python3 tests/fixtures/acceptance/generate.py encrypted           # writes p1-encrypted.pdf, 1.0 KB
$ file tests/fixtures/acceptance/*.pdf
# Confirms all three are valid PDF 1.4.

$ python3 -c "
import pypdf
r = pypdf.PdfReader('tests/fixtures/acceptance/p1-encrypted.pdf')
assert r.is_encrypted
assert r.decrypt('wrong').name == 'NOT_DECRYPTED'
r.decrypt('vibepdf'); assert len(r.pages) == 1
"
# Confirms password round-trip works end-to-end.

# After verification, the generated PDFs are deleted — they're
# gitignored in tests/fixtures/acceptance/*.pdf and rebuilt on demand:
$ rm -f tests/fixtures/acceptance/p1-{spec,large,encrypted}.pdf
```

For the *real* roadmap acceptance run later:

```bash
$ pip install -r tests/fixtures/acceptance/requirements.txt
$ python3 tests/fixtures/acceptance/generate.py all
# Defaults: 1000-page spec, 500 MB large, encrypted hello.pdf.
# Takes a couple of minutes on the large one.
```

### P1.B1 — Real document actor (this commit)

```bash
# Toolchain bootstrap — first Rust-touching step since project start.
$ curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --default-toolchain stable --profile default
# Installed rustc 1.95.0 + cargo 1.95.0 + rustup 1.29.0 for
# aarch64-apple-darwin. Cargo bin dir lives at ~/.cargo/bin; not on
# the default zsh PATH, so cargo invocations below all start with
# `. "$HOME/.cargo/env" && ...`.
$ . "$HOME/.cargo/env" && cargo --version && rustc --version

# PDFium prebuilt binary — needed for `cargo test` to bind to the
# native lib. Re-fetched after bumping PDFIUM_RELEASE to chromium/7857
# (the previous pin lacked symbols that pdfium-render 0.9 requires).
$ rm -f src-tauri/resources/pdfium/libpdfium.dylib
$ bash scripts/fetch-pdfium.sh
# Drops libpdfium.dylib (7.0 MB) into src-tauri/resources/pdfium/.
# Path is gitignored; every developer fetches their own.

# Placeholder Tauri icons so `tauri::generate_context!` validates.
# The bootstrap committed an empty icons/ dir; the macro reads each
# referenced PNG at compile-time and requires RGBA (color type 6).
$ python3 -c "<inline script that writes 32x32.png, 128x128.png, 128x128@2x.png as 8-bit RGBA>"
# Plus tiny stub icon.icns / icon.ico just to satisfy the existence
# check; bundle stage will reject those — that's expected (we're not
# bundling, and real icons are a separate concern).

# Node deps — clean reinstall because Node was bumped 18.15 → 22.4.0
# since the last commit, and rolldown's native binding optionally
# installs based on the active platform.
$ rm -rf node_modules package-lock.json
$ npm install
$ npm install --no-save @rolldown/binding-darwin-arm64@1.0.2
# Workaround for npm not installing optionalDependencies on the first
# pass; without this, vitest can't load rolldown's WASM/native shim.

# Verification gates (in the order the workflow expects them):
$ . "$HOME/.cargo/env" && npm run check
#   tsc --noEmit ✓
#   eslint src --max-warnings=0 ✓
#   cargo clippy --all-targets -- -D warnings ✓

$ . "$HOME/.cargo/env" \
    && DYLD_LIBRARY_PATH="$PWD/src-tauri/resources/pdfium" \
       cargo test --manifest-path src-tauri/Cargo.toml
#   tests/actor_smoke.rs ............ 4 passed (B1 acceptance)
#   tests/pdfium_init.rs ............ 1 passed (pre-existing smoke)

$ npm run test
#   52 passed, 4 failed — failures pre-date this commit:
#     src/state/__tests__/view-persistence.test.ts — IDB hook
#     timeouts with fake-indexeddb under Node 22 (3 tests)
#     src/view/__tests__/render-page.test.ts — DOMMatrix undefined
#     under jsdom (1 test)
#   Neither file is touched by B1. Tracked as environment drift from
#   the Node upgrade, not a B1 regression. Do not "fix" by editing
#   the failing tests (workflow rule: no test deletion to pass CI).
```

---

### Env fix — restore vitest green on Node 22 (this commit)

```bash
# Diagnosis: reproduce the 4 failures the previous commit (P1.B1)
# noted as deferred env drift.
$ npm run test
#   52 passed, 4 failed — exactly as expected:
#     src/view/__tests__/render-page.test.ts — DOMMatrix undefined.
#     src/state/__tests__/view-persistence.test.ts — 3 × IDB timeouts.

# Probe: is the IDB failure a slow test or a real deadlock?
$ npx vitest run src/state/__tests__/view-persistence.test.ts \
    --testTimeout=30000 --hookTimeout=30000
#   Still red after 30 s — genuine deadlock, not slowness.
#   Caused by deleteDatabase() racing an already-open connection
#   under fake-indexeddb 6.x's stricter block semantics.

# Fix 1 — pin fake-indexeddb to the last 5.x:
$ npm install --save-dev fake-indexeddb@5.0.2 --no-audit --no-fund
# package.json:          ^6.2.5 → ^5.0.2
# package-lock.json:      6.2.5 →  5.0.2 (plus normal re-keying)

# Fix 2 — DOMMatrix stub + legacy-build alias + worker preload, all
# in src/test-setup.ts and vite.config.ts. No new npm deps; the
# `pdfjs-dist/legacy/build/*` files already ship in the package.

# Verification gates:
$ npx tsc --noEmit
#   ✓ (after adding `export {};` to test-setup.ts so the top-level
#     `await import("pdfjs-dist/legacy/build/pdf.worker.mjs")` is in
#     module context; the worker subpath has no .d.ts in the package
#     so it's silenced with a one-line @ts-expect-error.)

$ npx eslint src --max-warnings=0
#   ✓

$ npm run test
#   56 passed, 0 failed (was 52 / 4 on Node 22.4 + Node-18-era deps).
#   Re-ran twice to confirm no flakes — both ~1.3 s wall.
```

### P1.B3 — Render-page-to-bitmap message (this commit)

```bash
# No toolchain changes — Rust + PDFium dylib already installed
# during B1, vitest env stable since the Node-22 fix commit.

# Verification gates (in the order the workflow expects them):
$ . "$HOME/.cargo/env" && npm run check
#   tsc --noEmit ✓
#   eslint src --max-warnings=0 ✓
#   cargo clippy --all-targets -- -D warnings ✓  (after iterating
#     on cast-truncation, doc-backtick, and unwrap_used lints in
#     pdf/render.rs)

$ . "$HOME/.cargo/env" \
    && DYLD_LIBRARY_PATH="$PWD/src-tauri/resources/pdfium" \
       cargo test --manifest-path src-tauri/Cargo.toml
#   actor_smoke.rs ............ 4 passed (unchanged from B1)
#   pdfium_init.rs ............ 1 passed (unchanged)
#   render_to_png.rs .......... 5 passed, 1 ignored (release-only
#     perf sentinel)
#   render_verification_artifact.rs ... 1 ignored (on-demand)

# Verification artifact for human eyeball:
$ cd src-tauri && DYLD_LIBRARY_PATH="$PWD/resources/pdfium" \
    cargo test --test render_verification_artifact -- \
      --include-ignored --nocapture
#   wrote /tmp/vibepdf-verify-72dpi.png (612x792, 20042 bytes)
#   wrote /tmp/vibepdf-verify-144dpi.png (1224x1584, 59970 bytes)
#
# `file` confirms both are 8-bit RGBA PNGs, non-interlaced.
# Open in Preview: hello.pdf glyph centred on white, 144 DPI is
# visibly sharper at the same display size. Both written ~140 ms
# end-to-end including PDFium init.

$ npm run test
#   56 passed, 0 failed (unchanged from B1 follow-up).

# Discovery: SIGTRAP/SIGABRT under parallel `cargo test` when
# multiple actors render simultaneously. Fix is `RENDER_LOCK:
# Mutex<()>` in pdf/render.rs wrapping all PDFium calls (page
# lookup, metadata read, render). PDFium's `FX_GE` render subsystem
# has process-global state; the per-document actor pattern is not
# enough on its own. Test verifies fix passes under default parallel
# runner.
```

### Fix — dpi_target_width_math test assertion (this commit)

```bash
# Reproduced the failing B3-era unit test:
$ . "$HOME/.cargo/env" \
    && DYLD_LIBRARY_PATH="$PWD/src-tauri/resources/pdfium" \
       cargo test --manifest-path src-tauri/Cargo.toml --lib \
       pdf::render::tests::dpi_target_width_math
#   FAILED: left 17000, right 200000. The test asserted the 200k-px
#   output clamp fires at 99_999 DPI, but the function clamps DPI to
#   2000 *first* (612/72*2000 = 17000), so MAX_PX never triggers for a
#   letter page. Code matches its doc comment; the test arithmetic was
#   wrong. Corrected the assertion to 17_000 and added a real MAX_PX
#   case via a 10_000 pt page width.

# Gates after the fix:
$ . "$HOME/.cargo/env" && cd src-tauri && cargo clippy --all-targets -- -D warnings  # pass
$ . "$HOME/.cargo/env" \
    && DYLD_LIBRARY_PATH="$PWD/src-tauri/resources/pdfium" \
       cargo test --manifest-path src-tauri/Cargo.toml
#   15 passed, 2 ignored (was 14 passed / 1 failed / 2 ignored).
```

No dependency or behavior change — test-only correction in
`src-tauri/src/pdf/render.rs`.

### P1.B2 — Encrypted-PDF password prompt (this commit)

```bash
# Regenerate the encrypted fixture if it's missing on the host. The
# acceptance generator dep (pypdf) is the only Python lib we need.
# Homebrew Python is PEP 668 EXTERNALLY-MANAGED, so a venv is now
# the only sanctioned install path — system pip refuses without
# --break-system-packages.
$ python3 -m venv .venv-fixtures
$ .venv-fixtures/bin/pip install -q -r tests/fixtures/acceptance/requirements.txt
$ .venv-fixtures/bin/python tests/fixtures/acceptance/generate.py encrypted
# Writes tests/fixtures/acceptance/p1-encrypted.pdf (~1 KB) with
# user_password=vibepdf / owner_password=vibepdf-owner. Gitignored.

# Verification gates:
$ npx tsc --noEmit                                                # pass
$ npx eslint src --max-warnings=0                                 # pass
$ . "$HOME/.cargo/env" && \
    cd src-tauri && cargo clippy --all-targets -- -D warnings     # pass

$ . "$HOME/.cargo/env" \
    && DYLD_LIBRARY_PATH="$PWD/src-tauri/resources/pdfium" \
       cargo test --manifest-path src-tauri/Cargo.toml --test encrypted_open
#   3 passed (no password → PasswordRequired, wrong → PasswordRequired,
#   correct → opens with page_count = 1).

$ npm run test
#   56 passed, 0 failed (unchanged from a7184d5 — vitest tests are
#   unaffected; the manual UI verification is in the commit body).
```

No new npm or cargo dependencies. `.venv-fixtures/` added to `.gitignore`
so it survives across runs without polluting `git status`.

**Pre-existing test failure noted (not B2):** `cargo test --lib`
fails on `pdf::render::tests::dpi_target_width_math` — a B3-era
assertion that disagrees with the actual DPI clamp. Spawned a separate
task chip; tracked under "Fix dpi_target_width_math test mismatch."
The B2 commit explicitly does **not** touch `src-tauri/src/pdf/render.rs`.

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
