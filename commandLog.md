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

### P1.A3 — Recents (last 20, clearable, persisted) (this commit)

```bash
# Verification gates:
$ npx tsc --noEmit                                                # pass
$ npx eslint src --max-warnings=0                                 # pass
$ . "$HOME/.cargo/env" && \
    (cd src-tauri && cargo clippy --all-targets -- -D warnings)   # pass
#   (one fix: #[must_use] on settings::recents::load)

$ . "$HOME/.cargo/env" \
    && DYLD_LIBRARY_PATH="$PWD/src-tauri/resources/pdfium" \
       cargo test --manifest-path src-tauri/Cargo.toml --test recents
#   6 passed: dedup-to-front, cap-at-20, disk round-trip,
#   missing-file→empty, corrupt-file→empty, save-then-clear.

$ . "$HOME/.cargo/env" \
    && DYLD_LIBRARY_PATH="$PWD/src-tauri/resources/pdfium" \
       cargo test --manifest-path src-tauri/Cargo.toml
#   21 passed, 2 ignored (added 6 recents to the prior 15).

$ npm run test
#   56/56 (unchanged — recents store rewrite has no vitest yet; UI is
#   verified manually per the commit body).
```

No new npm or cargo dependencies (tests reuse the existing `uuid`
crate for temp paths instead of pulling in `tempfile`). Recents persist
to `<app_data_dir>/recents.json`; on macOS that's
`~/Library/Application Support/<bundle-id>/recents.json`.

### P1.E1 — Multi-document tab/session restore (this commit)

```bash
# Verification gates:
$ npx tsc --noEmit                                                # pass
$ npx eslint src --max-warnings=0                                 # pass
$ . "$HOME/.cargo/env" && \
    (cd src-tauri && cargo clippy --all-targets -- -D warnings)   # pass
#   (one fix: backtick `IndexedDB` in a session.rs doc comment.)

$ . "$HOME/.cargo/env" \
    && DYLD_LIBRARY_PATH="$PWD/src-tauri/resources/pdfium" \
       cargo test --manifest-path src-tauri/Cargo.toml --test session_restore --test recents
#   session_restore: 5 passed. recents: 6 passed (regression guard for
#   the settings:: shared-helper refactor).

$ . "$HOME/.cargo/env" \
    && DYLD_LIBRARY_PATH="$PWD/src-tauri/resources/pdfium" \
       cargo test --manifest-path src-tauri/Cargo.toml
#   26 passed, 2 ignored (added 5 session_restore to the prior 21).

$ npm run test
#   56/56 (unchanged — restore/persist UI is verified manually).
```

No new npm or cargo dependencies. Session persists to
`<app_data_dir>/session.json`; reuses the atomic-write + defensive-read
helpers lifted into `settings/mod.rs` from A3's recents.

### P1.A2 — CLI-arg file open (this commit)

```bash
# Verification gates:
$ npx tsc --noEmit                                                # pass
$ npx eslint src --max-warnings=0                                 # pass
$ . "$HOME/.cargo/env" && \
    (cd src-tauri && cargo clippy --all-targets -- -D warnings)   # pass

$ . "$HOME/.cargo/env" \
    && DYLD_LIBRARY_PATH="$PWD/src-tauri/resources/pdfium" \
       cargo test --manifest-path src-tauri/Cargo.toml --test cli_open
#   6 passed: case-insensitive .pdf, drop-argv0-and-non-pdf, order
#   preservation, empty input, only-argv0, .pdf boundary.

$ . "$HOME/.cargo/env" \
    && DYLD_LIBRARY_PATH="$PWD/src-tauri/resources/pdfium" \
       cargo test --manifest-path src-tauri/Cargo.toml
#   32 passed, 2 ignored (added 6 cli_open to the prior 26).

$ npm run test
#   56/56 (unchanged).
```

No new npm or cargo dependencies. CLI args buffer in `AppState.cli_pending`
during `setup`; the frontend drains via `cli_take_pending_opens` at the
tail of the session-restore IIFE.

### Review follow-up — shared basename + password-loop tests (this commit)

```bash
# Quality fixes from a post-A2 review pass (no spec step):
#   - consolidate duplicated basename → src/app/paths.ts
#   - add src/app/__tests__/open-with-password.test.ts (P1-VIEW-003)

$ npx tsc --noEmit                                                # pass
$ npx eslint src --max-warnings=0                                 # pass
$ . "$HOME/.cargo/env" && \
    (cd src-tauri && cargo clippy --all-targets -- -D warnings)   # pass (no Rust change)

$ npm run test
#   63/63 (was 56 — +7 open-with-password cases).
```

No dependency or production-behaviour change beyond unifying `basename`.

### Review follow-up — split App.tsx into hooks (this commit)

```bash
# Behaviour-preserving refactor (no spec step): extract useFileOpen +
# useSessionRestore from App.tsx (354 → 128 lines).

$ npx tsc --noEmit                                                # pass
$ npx eslint src --max-warnings=0                                 # pass
#   (react-hooks/exhaustive-deps re-validates every moved effect)
$ . "$HOME/.cargo/env" && \
    (cd src-tauri && cargo clippy --all-targets -- -D warnings)   # pass (no Rust change)

$ npm run test
#   63/63 (unchanged — logic moved verbatim).
```

No dependency or behaviour change. App.tsx wiring remains
manually-verified (no vitest for it yet; that's E5's job).

### P1.D1 — Thumbnails sidebar (this commit)

```bash
# No new deps (reuses B3's renderPage wrapper + the existing
# fake-indexeddb test dep). No Rust change.

# Verification gates:
$ npx tsc --noEmit                                                # pass
#   (one fix: copy Uint8Array into a fresh ArrayBuffer for Blob —
#    TS 5.7 won't widen Uint8Array<ArrayBufferLike> to BlobPart.)
$ npx eslint src --max-warnings=0                                 # pass
#   (added `Blob` to eslint.config.js globals — Web API, not an ES
#    builtin, so no-undef flagged it.)
$ . "$HOME/.cargo/env" && \
    (cd src-tauri && cargo clippy --all-targets -- -D warnings)   # pass (no Rust change)

$ npm run test
#   67/67 (was 63 — +4 thumbnail-cache IDB tests).
```

cargo test unchanged at 32/2 (no Rust touched).

### P1.E2 — Render-failure log scaffold (this commit)

```bash
# No new deps (png decoder already present; no Cargo change). No
# frontend change.

# Generate the committed golden once (and after any intentional
# renderer change):
$ . "$HOME/.cargo/env" \
    && DYLD_LIBRARY_PATH="$PWD/src-tauri/resources/pdfium" \
       cargo test --manifest-path src-tauri/Cargo.toml --test render_compare \
       -- --ignored bless_goldens
# Wrote tests/fixtures/golden/hello-p0-72dpi.png (612×792 RGBA, ~20 KB).

# Gate (renders match goldens; writes tests/render-failures.md):
$ . "$HOME/.cargo/env" \
    && DYLD_LIBRARY_PATH="$PWD/src-tauri/resources/pdfium" \
       cargo test --manifest-path src-tauri/Cargo.toml --test render_compare
# renders_match_goldens ... ok (no divergences). Re-running produces a
# byte-identical render-failures.md (verified by shasum) → git stays
# clean on a passing run.

# Verification gates:
$ npm run check                                                   # clean (tsc+eslint+clippy)
$ . "$HOME/.cargo/env" \
    && DYLD_LIBRARY_PATH="$PWD/src-tauri/resources/pdfium" \
       cargo test --manifest-path src-tauri/Cargo.toml
# 33 passed, 3 ignored (added render_compare's gate; bless is ignored).
$ npm run test                                                    # 67/67 (unchanged)
```

Manual: opened tests/fixtures/golden/hello-p0-72dpi.png — renders
"Hello, VibePDF." (confirms the golden is a real render, not noise).

### Infra — CI + Rust test scripts (this commit)

```bash
# Discovered npm run test:rust was broken (no PDFium on the loader
# path → LoadLibraryError):
$ npm run test:rust          # before: actor/encrypted/render tests FAILED

# Fix: scripts/cargo-test.mjs sets DYLD_/LD_LIBRARY_PATH (or PATH on
# win) to src-tauri/resources/pdfium, then runs cargo test. Wired into
# both test:rust and the new test:pdf.
$ npm run test:pdf
#   render_compare + render_to_png + actor_smoke + encrypted_open:
#   13 passed.
$ npm run test:rust
#   33 passed total (full Rust suite, was failing before).

# Added .github/workflows/ci.yml (macos-latest — matches the E2 render
# golden's platform; no webkit apt deps needed). Verified its commands
# locally:
$ npm run check              # clean (tsc + eslint + clippy)
$ npm run test               # 67/67 (vitest)
# YAML structure validated via js-yaml.

# NOTE: GitHub Actions itself was not run here — the workflow gets its
# first real run on push/PR. Commands within it are all green locally.
```

No new dependencies. CLAUDE.md command table + "done" criteria
corrected to match the real scripts.

### Bug fix — PDF.js worker asset missing from public/ (this commit)

```bash
# Symptom (real Tauri window, first GUI test): every PDF →
# "Setting up fake worker failed: Importing a module script failed."
# Root cause: workerSrc = /pdfjs/pdf.worker.min.mjs but public/ was empty.

# Fix: copy the worker from node_modules into public/ (gitignored),
# automated via npm hooks.
$ node scripts/copy-pdfjs-worker.mjs
#   → public/pdfjs/pdf.worker.min.mjs (1.2 MB)

# Verification:
$ npm run check      # clean
$ npm run test       # 68/68 — incl. new pdfjs-worker asset-presence guard.
#   (pretest hook runs the copy, so the test sees the file in CI too.)
```

No new dependencies. Generated `public/pdfjs/` gitignored. Full GUI
verification (does a PDF actually render now) is on the human — re-run
`npm run dev` and open a PDF via the in-app ⌘O button.

### Bug fix — thumbnail bytes + HiDPI rendering (this commit)

```bash
# Found in GUI testing (after the worker fix let PDFs render):
#   - thumbnail sidebar: ⚠ on every page
#   - main view: blurry text
#
# Causes:
#   - pdf_render_page's Vec<u8> arrives over IPC as number[] (not
#     Uint8Array); thumbnail code read .byteLength (undefined) → throw.
#   - renderPageOnDoc ignored devicePixelRatio → 1× bitmap on a 2×
#     screen → blur.

$ npm run check     # clean (tsc + eslint + clippy)
$ npm run test      # 68/68
```

No new dependencies. GUI verification (thumbnails render, text is
crisp) is on the human — re-run `npm run dev`, open a PDF.

### Theme toggle UI (this commit)

```bash
# Wires a Light/Dark/System <select> in the toolbar to the existing
# theme machinery (P1-VIEW-010). Lets the user turn off the dark-mode
# page invert instead of being forced by the OS.
$ npm run check     # clean
$ npm run test      # 68/68
```

No new dependencies. Frontend-only. GUI verification (switching to Light
gives a white page + crisp text) is on the human.

### Bug fix — dark-mode invert: pixel → CSS filter (this commit)

```bash
# Light mode crisp (HiDPI fix), dark mode "pixelated / black on black"
# — the per-pixel invert heuristic mangled text. Swapped to a CSS
# filter on the canvas (compositor invert at native resolution).
# Removed the dead dark-invert.ts + its 8 tests.

$ npm run check     # clean
$ npm run test      # 60/60 (was 68; -8 dead pixel-invert tests)
```

No new dependencies. GUI verification (dark mode crisp now) is on the
human.

### Test hardening — component/integration tests (this commit)

```bash
# First use of @testing-library/react (already installed). Adds tests
# for the GUI-bug class this session surfaced: theme wiring, HiDPI
# sizing, thumbnail number[] bytes.

# Verified each guard has teeth by reintroducing the bug:
$ perl -0pi -e 's/scale: input\.scale \* dpr/scale: input.scale/' src/view/render-page.ts
$ npm run test -- src/view/__tests__/render-page-hidpi.test.ts   # RED (400≠800)
$ git checkout src/view/render-page.ts
# (same for ThumbnailPanel: number[]→byteLength bug → "Unable to find img")

# Gates:
$ npm run check     # clean (after adding HTMLSelectElement +
#                     IntersectionObserver{Callback,Entry} to eslint globals)
$ npm run test      # 67/67 (was 60; +7 component/integration tests)
```

No new dependencies.

### P1.E5 — E2E harness (this commit)

```bash
# WebdriverIO + tauri-driver (NOT Playwright — it can't drive a Tauri
# webview). Linux/Windows only; written blind from macOS.

$ npm install -D @wdio/cli@^9 @wdio/local-runner@^9 \
    @wdio/mocha-framework@^9 @wdio/spec-reporter@^9 tsx
# 412 dev-only packages.

# Locally validated (all I can do on macOS):
$ npx tsc -p tests/e2e/tsconfig.json --noEmit   # e2e sources typecheck
$ npm run check                                  # app gates unaffected (src only)
$ npm run test                                   # 67/67 (vitest unaffected)
$ npx wdio --version                             # 9.27.2
# e2e.yml validated as YAML (js-yaml).

# NOT run here — needs Linux + tauri-driver + webkit + xvfb + a built
# app. The real gate is the e2e.yml run on CI:
#   cargo install tauri-driver --locked
#   npx tauri build --debug --no-bundle
#   LD_LIBRARY_PATH=.../resources/pdfium xvfb-run -a npm run test:e2e
```

No app-code change. New dev deps only (@wdio/*, tsx; tauri-driver is a
cargo-installed CI binary). Step left `[~]` until the first CI run goes
green.

---

### P2.A1 — Save (explicit Cmd/Ctrl+S) (this commit)

```bash
# No new deps. pdfium-render already exposes save_to_bytes()/save_to_file();
# the write path uses std::fs only. First byte-writing feature in the repo.

# Verification gates (workflow order):
npm run check
#   tsc --noEmit ✓
#   eslint src --max-warnings=0 ✓
#   cargo clippy --all-targets -- -D warnings ✓
#     (one iteration: clippy::pedantic doc_markdown wanted `PDFium`
#      backticked in 6 new doc comments.)

npm run test
#   70/70 (was 67 — +3 src/ipc/__tests__/save.test.ts: path?? null
#   marshalling + SaveOutcome passthrough).

npm run test:rust
#   save_noop.rs: 3 passed —
#     save_as_roundtrips_page_count   (write+verify path)
#     save_same_path_not_dirty_is_true_noop  (byte-identical no-op)
#     save_document_rotates_bak_when_overwriting  (.bak rotation)
#   Full Rust suite green; no regressions in touched modules
#   (actor_smoke, render_*, session_restore).

# PDF write-path verification ritual (ignored test, run on demand):
node scripts/cargo-test.mjs --test save_noop \
  save_writes_verification_artifact -- --ignored --nocapture
#   → /tmp/vibepdf-verify.pdf
file /tmp/vibepdf-verify.pdf
#   PDF document, version 1.4, 1 pages (zip deflate encoded), 693 bytes.
#   (Note: 693 B ≠ original hello.pdf size — PDFium re-serializes, which
#    is exactly why the same-path no-op must skip the write entirely.)
```

Awaiting the human cross-reader check (Acrobat / Preview / a third
reader) before `steps/P2.md` A1 flips `[~]` → `[x]`.

(Human verified the artifact in Preview/Acrobat + re-opened it in VibePDF;
A1 marked `[x]`.)

---

### Bug fix — thumbnail sidebar dark mode (this commit)

```bash
# Reported in the dev GUI: dark mode inverts the main page view but the
# thumbnail sidebar stays light. Root: darkMode never reached
# ThumbnailPanel. Shared DARK_PAGE_FILTER + threaded the prop through.
# No new deps, no Rust change.

npm run check
#   tsc ✓  eslint ✓ (added HTMLImageElement to DOM globals for the test
#   cast)  clippy ✓ (Rust untouched).

npm run test
#   71/71 (was 70 — +1 ThumbnailPanel dark-mode filter assertion; the
#   existing light-mode test also now asserts no filter).
```

Visual confirmation is the human's (re-run `npm run dev`): sidebar
thumbnails now invert with the page when dark mode is on.

---

### P2.A3 — Undo/redo stack (this commit)

```bash
# No new deps. Generic UndoStack<T> + Edit<T> command pattern; the actor
# holds UndoStack<PdfDocument>, empty until P2.B* adds real edits.
# NOT a PDF write path (undo/redo on an empty stack are no-ops; nothing is
# saved), so no /tmp verification artifact this step.

npm run check
#   tsc ✓  eslint ✓  clippy --all-targets -D warnings ✓
#   (one fix: the in-lib #[cfg(test)] module is the first in src/, so it
#    trips the crate-level clippy::unwrap_used / expect_used warns —
#    added #[allow(clippy::unwrap_used, clippy::expect_used)] on mod tests,
#    the idiomatic exemption for test code.)

npm run test
#   76/76 (was 71 — +5 history-store action tests).

npm run test:rust
#   pdf::undo unit tests: 4 passed (undo/redo round-trip, redo-cleared-on-
#     new-edit, empty-stack no-op, depth cap drops oldest).
#   tests/undo_redo.rs: 2 passed (fresh doc empty history; no-op undo/redo
#     leave page count untouched).
#   Full Rust suite green; no regressions (actor_smoke, save_noop, render_*).
```

Step left `[~]`: the acceptance ("delete pages → undo → restored") needs a
real Edit<PdfDocument>, which lands with P2.B2 (delete).

---

### P2.A2 — Auto-save + crash recovery (this commit)

```bash
# No new deps (no tokio "time" feature added — the 30s tick is a std
# thread). Autosave bytes go through doc.save_to_bytes() — the SAME
# serialization A1's save uses, already cross-reader-verified — so no
# separate /tmp artifact this step.

npm run check
#   tsc ✓  eslint ✓  clippy --all-targets -D warnings ✓
#   (two fixes: unix_now map().unwrap_or() -> map_or(); run_worker grew
#    past the 100-line lint with the new arm -> #[allow(too_many_lines)]
#    on the worker, which is just one big message-dispatch match.)

npm run test
#   77/77 (was 76 — +1 use-recovery hook test: surfaces entries, recover
#   opens+drops, discard drops only).

npm run test:rust
#   tests/autosave.rs: 4 passed —
#     write_then_scan_round_trips (copy re-opens in PDFium; sidecar keeps
#       the original path), discard idempotent, scan skips orphaned/
#       malformed, scan of a missing dir is empty.
#   Full Rust suite green; no regressions.
```

Step left `[~]`: the live write only fires for a *dirty* doc, and the
"force-kill → relaunch → recover" demo needs a real edit — both land with
P2.B2 (delete).

---

### P2.B1 — Rotate page(s) (this commit)

```bash
# No new deps. First real edit: PDFium /Rotate via RotateEdit<PdfDocument>,
# wired through undo (A3) + dirty (A1/A2). PDF WRITE PATH.

npm run check
#   tsc ✓  eslint ✓  clippy --all-targets -D warnings ✓
#   (fix: backtick `PDFium` in 4 new doc comments — pedantic doc_markdown.)

npm run test
#   81/81 (was 77 — +2 ipc/rotate, +2 thumbnail-cache deleteThumb).

npm run test:rust
#   rotate.rs: 4 passed (+1 ignored artifact) — rotate persists through
#     save/reopen, rotate→undo restores, redo re-applies, out-of-range is a
#     typed error that records nothing (atomic). Full suite green.

# --- concurrency bug surfaced + fixed ---
# Rotating under cargo's parallel runner SIGABRT'd, then SIGSEGV'd: PDFium
# is unsafe across documents (page-ops/save/FPDF_CloseDocument race global
# state). Fixes:
#   - one process-global pdf::document::PDFIUM_LOCK around ALL PDFium FFI
#     (was a render-only lock); close the actor's doc under it.
#   - scripts/cargo-test.mjs now runs the harness --test-threads=1 (tests
#     open/drop their own docs and can't take the pub(crate) lock).

# PDF write-path verification ritual (ignored test, run on demand):
node scripts/cargo-test.mjs --test rotate \
  rotate_writes_verification_artifact -- --ignored --nocapture
#   → /tmp/vibepdf-verify-rotated.pdf (also copied to ~/Desktop)
file /tmp/vibepdf-verify-rotated.pdf
#   PDF document, version 1.4, 1 pages — page 0 /Rotate = 90°.
```

Step left `[~]` pending the human cross-reader check (Acrobat / Preview /
a third reader) of the rotated artifact.

---

### Edit-preview pipeline (this commit)

```bash
# No new deps. Makes edits show live in the main view + thumbnails via a
# per-doc "edit epoch" signal + reload-from-actor-bytes (pdf_get_bytes).
# NOT a new write path — get_bytes is read-only (same save_to_bytes the
# already-verified save uses); persistence is still B1's save.

npm run check
#   tsc ✓  eslint ✓ (added requestAnimationFrame to DOM globals)  clippy ✓

npm run test
#   83/83 (was 81 — +2 edit-epoch-store).

npm run test:rust
#   get_bytes.rs: 2 passed — live bytes reopen, and reflect an UNSAVED
#   in-memory rotation (the whole point). Full suite green.
```

GUI-heavy: the live reload + no-blank swap + page restore can only be
confirmed in the real app (npm run dev). Not a steps/P2 item — implements
the BACKLOG "live edit-preview pipeline"; that item removed, a perf-
optimization item added in its place.

---

### P2.B2 — Delete page(s) (this commit)

```bash
# No new deps. DeleteEdit<PdfDocument> + content-preserving inverse
# (holding doc via create_new_pdf + FPDF_ImportPages). PDF WRITE PATH.

# New fixture for the reference-integrity test (pure stdlib, committed):
python3 tests/fixtures/basic/generate-links.py
#   → tests/fixtures/basic/links.pdf (3 pages; page-1 link → page-3 object)

npm run check
#   tsc ✓  eslint ✓  clippy --all-targets -D warnings ✓
#   (fix: backtick `PDFium` in 3 new doc comments.)

npm run test
#   85/85 (was 83 — +2 ipc/delete-pages).

npm run test:rust
#   delete_page.rs: 5 passed (+1 ignored artifact) —
#     count drop+persist, undo restores count+ORDER, redo, OUT-OF-RANGE
#     atomic error, and the CANARY:
#       surviving_link_target_stays_correct — delete page 2, the page-1
#       link still resolves to page 3 (now index 1). Object-refs survive
#       renumbering → spec's "update internal references" holds for
#       surviving targets WITHOUT any active rewrite.
#   delete_page unit tests (range_string / validate): passed in-lib.

# PDF write-path verification artifact (ignored test, run on demand):
node scripts/cargo-test.mjs --test delete_page \
  delete_writes_verification_artifact -- --ignored --nocapture
#   → /tmp/vibepdf-verify-deleted.pdf (2 pages; also copied to ~/Desktop)
```

Left `[~]` pending the human cross-reader check + GUI flow. Active
reference rewriting (dangling refs, reorder) deferred → BACKLOG (lopdf).

---

### Bug fixes — viewer regressions + pinch zoom (this commit)

```bash
# Found by driving the real app (npm run dev). No test coverage — the page
# virtualizer needs a real DOM/canvas/IntersectionObserver/PDF.js.
#   1. doc switch kept the old page (no-blank swap + StrictMode left
#      PageVirtualizer mounted) → setDoc(null) on switch, blank-free on edit.
#   2. trackpad pinch didn't zoom → Ctrl/Cmd+wheel listener → setZoom.
#   3. rotate 180° only updated the thumbnail (same dims → stale cache key)
#      → put the edit epoch in the page-render cache key.

npm run check    # tsc ✓  eslint ✓ (added WheelEvent global)  clippy ✓
npm run test     # 85/85 (unchanged — viewer has no unit tests)
```

GUI-verified by the human after re-running `npm run dev`.

---

### Bug fixes round 2 — reliable reload, zoom, close tabs (this commit)

```bash
# More GUI bugs from real use. Common thread for the "stale view / invalid
# pdf / switch-tab-fixes-it" class: the no-blank in-place doc swap was
# unreliable under StrictMode. Reverted to a clean reload (clear `doc` →
# remount the virtualizer) on EVERY (re)load; each effect owns one doc and
# destroys it on cleanup (no shared-ref destroy races). Page position
# restored via an `initialPage` prop the virtualizer scrolls to once
# measured (replaces the racy rAF). Measurement now parallel (Promise.all).
#
#   - zoom crawled: the scale ref went stale between renders during a pinch
#     burst → accumulate immediately in the wheel handler + clamp deltaY.
#   - no way to close a tab → × button on each tab (closeDoc + closePdf).
#
# Deferred (BACKLOG): external-edit/file-watch reload; the no-blank +
# incremental-preview perf pass (big docs now blank+reparse per edit).

npm run check    # tsc ✓  eslint ✓  clippy ✓
npm run test     # 85/85 (viewer still has no unit coverage)
```

---

### Perf — rotate viewport fast-path (this commit)

```bash
# Make the common edit (rotate) instant even on a 1300-page book: instead
# of reloading the whole document, preview the rotation cosmetically via
# PDF.js getViewport({ rotation }) while PDFium keeps the real /Rotate.
#
#   - rotation-preview-store.ts: cosmetic per-(doc,page) rotation; reset on
#     any reload (reloaded bytes carry the real rotation).
#   - render-page.ts: renderPageOnDoc forwards `rotation` to getViewport.
#   - PageVirtualizer: per-page rotation → swap layout dims + render + cache
#     key. PdfViewer: resetDoc on (re)load.
#   - ThumbnailPanel: rotate updates the cosmetic store (NOT the epoch → no
#     full reload); the page's thumbnail re-renders from PDFium.
#   delete / undo / redo still do the full reload (BACKLOG).

npm run check   # tsc ✓  eslint ✓  clippy ✓
npm run test    # 92/92 (+7: rotation-preview-store ×4, render rotation ×3)
```

GUI-verified by the human (rotate instant; undo/switch still reload).

---

### P2.B3 — Insert blank page (this commit)

```bash
# No new deps, no new fixture. Third page edit; inverse REUSES DeleteEdit
# (B2). PDF WRITE PATH.

npm run check   # tsc ✓  eslint ✓  clippy --all-targets -D warnings ✓
npm run test    # 97/97 (+3 ipc/insert-blank)
npm run test:rust
#   insert_blank.rs: 4 passed (+1 ignored artifact) —
#     count+persist, inherit adjacent dims (612×792), undo-removes/
#     redo-reinserts, prepend+append+out-of-range (atomic).
#   (One test fix: handle.page_count() is the CACHED open-time count and is
#    stale after an edit → use metadata_live() (re-reads). Logged the cache
#    staleness to BACKLOG.)

# PDF write-path verification artifact (ignored, run on demand):
node scripts/cargo-test.mjs --test insert_blank \
  insert_writes_verification_artifact -- --ignored --nocapture
#   → /tmp/vibepdf-verify-inserted.pdf (4 pages; also ~/Desktop)
file /tmp/vibepdf-verify-inserted.pdf   # PDF 1.4, 4 pages
```

Left `[~]` pending the human cross-reader check + GUI flow.

---

### P2.B4 — Crop page (this commit)

```bash
# No new deps. CropBox-only crop (content untouched), reset-to-MediaBox,
# undoable. Margins dialog (drag-select deferred). PDF WRITE PATH.

npm run check   # tsc ✓  eslint ✓  clippy --all-targets -D warnings ✓
npm run test    # 100/100 (+3 ipc/crop)
npm run test:rust
#   crop.rs: 5 passed (+1 ignored artifact) — set+persist, reset→MediaBox,
#   undo restores prior box, out-of-range, inverted-rect rejected.
#   (Gotcha: pdfium-render crop() ERRORS when a page has no explicit
#    CropBox → fall back to media() for the effective box.)

# PDF write-path artifact (ignored, on demand):
node scripts/cargo-test.mjs --test crop \
  crop_writes_verification_artifact -- --ignored --nocapture
#   → /tmp/vibepdf-verify-cropped.pdf (page 1 cropped; also ~/Desktop)
```

Left `[~]` pending the human cross-reader check + GUI flow.

---

### P2.C2 — Extract pages to a new PDF (this commit)

```bash
# No new deps. First Track-C feature + first that WRITES A NEW FILE (not an
# edit → no undo/dirty). Reuses create_new_pdf + copy_pages_from_document
# (from delete's undo) + save_document (A1 verified write). PDF WRITE PATH.

npm run check   # tsc ✓  eslint ✓  clippy --all-targets -D warnings ✓
#   (one fix: exactOptionalPropertyTypes — ZoomToolbar onExtract typed
#    `(() => void) | undefined` to allow the no-doc case.)
npm run test    # 108/108 (+8: page-range ×6, ipc/extract ×2)
npm run test:rust
#   extract.rs: 3 passed (+1 ignored artifact) — selected pages, single/all,
#   out-of-range + empty write nothing. Full suite green.

# PDF write-path artifact (ignored, on demand):
node scripts/cargo-test.mjs --test extract \
  extract_writes_verification_artifact -- --ignored --nocapture
#   → /tmp/vibepdf-verify-extracted.pdf (2 pages; also ~/Desktop)
```

Left `[~]` pending the human cross-reader check. Unblocks C3 (split).

---

### P2.C3 — Split (4 modes) (this commit)

```bash
# No new deps. Read-only on the source (like C2 — no undo/dirty). Reuses the
# C2 writer, refactored to a shared extract::write_subset_pdf. PDF WRITE PATH
# (emits N files). New fixture for the bookmarks mode:
python3 tests/fixtures/basic/generate-bookmarks.py
#   → tests/fixtures/basic/bookmarks.pdf (6 pp, 3 top-level bookmarks @ 0/2/4)

npm run check   # tsc ✓  eslint ✓  clippy --all-targets -D warnings ✓
#   (two clippy fixes in split.rs: cast_sign_loss on step_by — use
#    usize::try_from; doc_markdown — backtick `PDFium`.)
npm run test    # 117/117 (+9: split-points ×6, ipc/split ×3)
npm run test:rust
#   split.rs: 6 passed (+1 ignored artifact) — every-N, at-pages, by-size
#   (1-byte → per-page; huge → <2-files error), by-bookmarks, bad-input.
#   Full suite green, no PDFium crashes (--test-threads=1).

# PDF write-path artifacts (ignored, on demand):
cargo test --test split split_writes_verification_artifacts \
  -- --ignored --test-threads=1 --nocapture   # (run from src-tauri/)
#   → /tmp/vibepdf-verify-split-00{1,2,3}.pdf (3×2 pp; also copied to ~/Desktop)
```

Left `[~]` pending the human cross-reader check. Unblocks C3-dependent work
(no further C-track item strictly depends on split, but it shares the
subset-writer with a future merge/D1).

---

### P2.C4 — Merge multiple PDFs (PARTIAL) (this commit)

```bash
# No new deps. PARTIAL against P2-PAGE-008: concat + page annotations now;
# bookmarks + form fields + collision renaming deferred (need lopdf). First
# STANDALONE command (no actor / no DocumentId) — docs/04 §"Stateless
# multi-file operations"; runs in spawn_blocking. PDF WRITE PATH.

npm run check   # tsc ✓  eslint ✓  clippy --all-targets -D warnings ✓
#   (one clippy fix: doc_markdown — backtick `PDFium` in the command doc.)
npm run test    # 126/126 (+9: merge/reorder ×7, ipc/merge ×2)
npm run test:rust
#   merge.rs: 6 passed (+1 ignored artifact) — concat page count, annotation
#   survival, order-respected, <2 guard, missing-file error, and
#   merge_does_not_yet_carry_bookmarks (tripwire locking the deferred gap).
#   Full suite green, no crashes (--test-threads=1).

# PDF write-path artifact (ignored, on demand):
cargo test --test merge merge_writes_verification_artifact \
  -- --ignored --test-threads=1 --nocapture   # (run from src-tauri/)
#   → /tmp/vibepdf-verify-merged.pdf (10 pp: bookmarks+links+hello); copied
#     to Sample PDFs/. Cross-checked: gs pdfpagecount=10 + visual page 1/7/10
#     render confirms in-order concat across the three sources.
```

Left `[~]` — only the concat+annotation leg of P2.C4 is done; the bookmark /
form-field / rename criteria stay open against P2-PAGE-008 pending the lopdf
decision.

---

### P2.D1 — Insert pages from another PDF (PARTIAL) (this commit)

```bash
# No new deps. PARTIAL against P2-PAGE-005: content + annotations + dimensions
# now; interactive form fields deferred (lopdf). Undoable actor edit
# (InsertFromEdit, inverse = DeleteEdit). Adds a read-only pdf_peek_page_count
# (standalone) so the dialog can show the source length. PDF WRITE PATH.

npm run check   # tsc ✓  eslint ✓  clippy --all-targets -D warnings ✓
#   (one clippy fix: doc_markdown — backtick `MediaBox`.)
npm run test    # 130/130 (+4: ipc/insert-from ×2, ipc/peek ×2)
npm run test:rust
#   insert_from.rs: 6 passed (+1 ignored artifact) — count+undo/redo,
#   annotation survival (save+reopen), start/end positions, validation
#   (bad source page / bad index, no mutation), missing-source, peek count.
#   Full suite green, no crashes (--test-threads=1).

# PDF write-path artifact (ignored, on demand):
cargo test --test insert_from insert_from_writes_verification_artifact \
  -- --ignored --test-threads=1 --nocapture   # (run from src-tauri/)
#   → /tmp/vibepdf-verify-insertfrom.pdf (4 pp: hello + links[1-3]); copied to
#     Sample PDFs/. Cross-checked: gs pdfpagecount=4 + page 1/2 render confirms
#     the import landed after hello (p1=hello, p2=links "Page 1").
```

Left `[~]` — content/annotations/dimensions done; the form-field clause of
P2-PAGE-005 stays open pending lopdf.

---

### lopdf adoption — COS layer + capability spike (this commit)

```bash
# NEW DEPENDENCY (approved): lopdf — pure-Rust COS/object-model library for the
# structural edits PDFium's API can't do. Needs network to fetch.
cargo add lopdf                       # resolved 0.36.0 (0.41 needs newer Rust
                                      # than our 1.80 floor)
# Then trimmed to default-features = false (drop chrono/jiff/time/rayon) in
# Cargo.toml by hand + justification comment.

# License audit of lopdf's transitive tree (plan requirement):
cargo tree -p lopdf --edges normal    # aes/cbc/ecb/md-5 (/Encrypt), flate2
                                      # (FlateDecode), nom, encoding_rs, indexmap
#   → all permissive (MIT/Apache/BSD); NO GPL/AGPL/LGPL. lopdf itself MIT.

# New AcroForm fixture for the form-field rename test:
python3 tests/fixtures/basic/generate-forms.py   # → forms.pdf (1 text field "name")

npm run check   # tsc ✓  eslint ✓  clippy --all-targets -D warnings ✓
#   clippy fixes in cos.rs: doc_markdown (backtick `PDFium`), manual_let_else,
#   single_match_else (match → let/if-let-else), needless_pass_by_value
#   (#[allow] on the cos_err map_err adapter).
npm run test    # 130/130 (unchanged — no frontend in this step)
npm run test:rust
#   cos.rs: 4 passed (+1 ignored artifact) — outline read; outline WRITE then
#   REOPEN IN PDFIUM; form-field rename then reopen; page count preserved.
#   The reopen-in-PDFium asserts prove cross-library byte compatibility (the
#   go/no-go gate). Full suite green (--test-threads=1).

# Optional cross-reader artifact (3rd engine check):
cargo test --test cos cos_writes_verification_artifact \
  -- --ignored --test-threads=1 --nocapture   # (from src-tauri/)
#   → /tmp/vibepdf-verify-lopdf.pdf (bookmark added to bookmarks.pdf); copied
#     to Sample PDFs/. gs pdfpagecount=6 → valid to Ghostscript too.
```

Decision LANDED. No user-facing feature; this validates the dependency and
unblocks C1 (reorder), C4 completion (bookmarks + form fields + rename), D1
completion (form fields), and B2/C3 dangling-ref cleanup — each its own step.

---

### P2.C1 — Reorder via thumbnail drag (this commit)

```bash
# No new deps. FIRST feature on the lopdf byte-handoff: ReorderEdit serializes
# the live doc → cos::reorder_pages (permute /Kids) → load_pdf_from_byte_vec
# REPLACES the actor's doc. Undoable (inverse permutation). PDF WRITE PATH.

npm run check   # tsc ✓  eslint ✓  clippy --all-targets -D warnings ✓
#   fixes: doc_markdown (backtick `PDFium` in reorder.rs); test helper
#   scoped the PdfPages borrow (avoid drop-of-non-Drop under -D warnings);
#   pages.get wants i32 (i32::try_from, not `as u16`).
npm run test    # 141/141 (+11: compute-reorder ×9, ipc/reorder ×2)
npm run test:rust
#   reorder.rs: 2 (+1 ignored) — annotated page moves + undo/redo (verified by
#   save+reopen+annotation position); permutation validation (no mutation).
#   cos.rs: +2 — reorder /Kids reopens in PDFium; rejects bad permutation.
#   Full suite green (--test-threads=1).

# PDF write-path artifact (ignored, on demand):
cargo test --test reorder reorder_writes_verification_artifact \
  -- --ignored --test-threads=1 --nocapture   # (from src-tauri/)
#   → /tmp/vibepdf-verify-reordered.pdf (links.pdf reordered [2,0,1]); copied
#     to Sample PDFs/. gs: 3 pages; render of p1 shows "Page 3" (old p3 → front).
```

Left `[~]` pending the human cross-reader check. Flat page tree only (nested =
BACKLOG). Object-ref reference integrity holds (the link page moves correctly).

---

### P2.C4 completion — full merge via lopdf (this commit)

```bash
# No new deps. ENGINE SWAP: merge now an all-lopdf merge (cos::merge_documents)
# replacing FPDF_ImportPages. Preserves bookmarks + form fields + renames
# colliding /T. PDF WRITE PATH (load merged bytes into PDFium → save_document).

npm run check   # tsc ✓  eslint ✓  clippy --all-targets -D warnings ✓
#   clippy fixes: explicit_iter_loop (`for (k,v) in dr` not `dr.iter()`);
#   doc_markdown (backtick `PDFium` in merge.rs).
npm run test    # 141/141 (no frontend change — engine swap is backend-only)
npm run test:rust
#   merge.rs: 7 (+1 ignored) — REGRESSION GUARDS kept green across the swap
#   (concat count, annotation survival, order, ≥2 guard, missing-file) PLUS
#   merge_carries_bookmarks (flipped tripwire; 6 top-level via PDFium) +
#   merge_carries_form_fields_with_rename (name + name_2 via cos read).
#   cos.rs: +1 (merge outlines+fields reopens in PDFium). Full suite green.

# PDF write-path artifact (ignored, on demand):
cargo test --test merge merge_writes_verification_artifact \
  -- --ignored --test-threads=1 --nocapture   # (from src-tauri/)
#   → /tmp/vibepdf-verify-merged.pdf (bookmarks.pdf + forms.pdf = 7 pp, 3
#     bookmarks + 1 form field); copied to Sample PDFs/. gs pdfpagecount=7.
#     (mdls showed 10 — stale Spotlight index of the overwritten file, not the
#     bytes; gs + PDFium both read 7.)
```

Full P2-PAGE-008 now met (bookmarks ✅ form fields ✅ collision rename ✅).
Left `[~]` pending the human cross-reader check (bookmarks panel + form field
in Acrobat/Preview).

---

### P2.D1 completion — form fields on insert (this commit)

```bash
# No new deps. HYBRID: keep the PDFium page-copy, add a lopdf pass
# (cos::register_inserted_form_fields) to re-attach inserted pages' terminal
# form fields into /AcroForm with /T collision rename. Inverse switched from
# DeleteEdit to the new generic RestoreDocEdit (byte-snapshot undo). WRITE PATH.

npm run check   # tsc ✓  eslint ✓  clippy --all-targets -D warnings ✓
#   clippy fix: doc_markdown (backtick `PDFium` in insert_from.rs).
npm run test    # 141/141 (no FE change)
npm run test:rust
#   insert_from.rs: 8 (+1 ignored) — NEW preserve-form-fields + collision-rename
#   PLUS regression guards (count + undo/redo now via RestoreDocEdit, annotation
#   survival, start/end, validation, missing-source) all green across the
#   engine change. cos.rs: +1 (register_inserted_form_fields, idempotent).
#   Fixed a false-collision bug: skip widgets already in /AcroForm /Fields.

# PDF write-path artifact (ignored, on demand):
cargo test --test insert_from insert_from_writes_verification_artifact \
  -- --ignored --test-threads=1 --nocapture   # (from src-tauri/)
#   → /tmp/vibepdf-verify-insertfrom.pdf (hello + links[1-3] + forms = 5 pp);
#     copied to Sample PDFs/. gs pdfpagecount=5; raw-byte grep confirms
#     /AcroForm + /Widget + /T (name) present (independent of our code).
```

Full P2-PAGE-005 now met (content/annotations/dimensions ✅ + form fields ✅).
Left `[~]` pending the human cross-reader check (fillable field in Acrobat).
Terminal fields only (kid hierarchies = follow-up).

---

### B2/C3 — dangling-reference cleanup on save (this commit)

```bash
# No new deps. cos::prune_dangling_destinations runs in save_document: removes
# broken /Link annotations + dangling bookmarks (re-chained), GCs orphans via
# prune_objects. No-op + infallible for clean docs. New /Square-annot fixture:
python3 tests/fixtures/basic/generate-annots.py   # → annots.pdf

npm run check   # tsc ✓  eslint ✓  clippy --all-targets -D warnings ✓
#   (test fixes: scoped PdfPages borrows to avoid drop-of-ref under -D warnings)
npm run test    # 141/141 (no FE change — backend write-path cleanup)
npm run test:rust
#   cos.rs: +2 (prune dead link / keep valid link). delete_page.rs: +2
#   (delete_prunes_dangling_link, delete_prunes_dangling_bookmark — REAL PDFium
#   dangling refs). split.rs: +1 (split_prunes_cross_file_link — dead link from
#   FPDF_ImportPages). insert_from preserves-annotations repointed to annots.pdf
#   (links.pdf's only annot is an internal link → import dangles → pruned).
#   Full suite green (107 rust tests, --test-threads=1).

# Key finding: lopdf's WRITER strips refs to deleted objects on save, so a
# dangling ref can only be manufactured via PDFium — hence the cos unit tests
# cover the "dead link" shape and the integration tests cover real dangling.

# PDF write-path artifact (ignored, on demand):
cargo test --test delete_page prune_writes_verification_artifact \
  -- --ignored --test-threads=1 --nocapture   # (from src-tauri/)
#   → /tmp/vibepdf-verify-pruned.pdf (bookmarks.pdf, page 3 deleted → Chapter 2
#     bookmark gone); copied to Sample PDFs/. Raw grep confirms (Chapter 2)
#     absent, (Chapter 1)/(Chapter 3) present; gs 5 pages.
```

Both halves of P2-PAGE-003 "update internal references" now done (surviving
refs track ✅, dangling refs pruned ✅). Same prune cleans C2/C3 outputs.
Named-dest cleanup → BACKLOG.

---

### P2.C1 (GUI fix) — thumbnail reorder via pointer events (this commit)

```bash
# No new deps. Frontend-only fix: P2.C1's drag-reorder never worked in the
# macOS Tauri webview (WKWebView). Diagnosed by instrumenting each DnD handler
# with console.warn and reading the live console: only dragstart + dragend
# fired — dragenter/dragover/drop NEVER fire in WKWebView. Rewrote the reorder
# with pointer events (pointerdown/move/up on the <ul>, 6px click/drag
# threshold, data-thumb-tile + elementFromPoint for the drop target).

# Verification (frontend only — no Rust changed, so clippy unaffected from the
# last green run; avoided running it concurrently with `npm run dev`'s cargo lock):
npx tsc --noEmit                       # ✓
npx eslint src --max-warnings=0        # ✓  (the `eslint src` scope from `npm run check`)
npx vitest run                         # 141/141 ✓ (reorder helper 9/9)

# Human-verified live in `npm run dev`: drag reorders pages (sidebar + main
# view), source tile dims + hovered tile rings, ⌘Z/⌘⇧Z work, plain click still
# selects. Tested on Sample PDFs/reorder-bisect-bookmarks.pdf (a flat-tree COPY,
# never the committed fixture).
```

Root cause + pattern recorded in `docs/04` "WebView quirks" and `Learning.md`.
**Do not use HTML5 DnD for any future in-app drag UI** — pointer events only.

---

### P2.B5 — resize page (this commit)

```bash
# No new deps. Resize scales content via the lopdf byte-handoff
# (cos::resize_pages wraps each page's content in `q <matrix> cm … Q` + sets the
# new /MediaBox), NOT PDFium.

# WHY the pivot (mid-implementation): the planned PDFium path
# (FPDFPage_TransFormWithClip / page.scale()) forces a reload_in_place
# (pdfium-render #93) that SIGSEGVs at process teardown — caught by running a
# single resize test in isolation:
cargo test --test resize resize_to_a4_sets_mediabox -- --test-threads=1
#   → signal: 11 (SIGSEGV) even though the assertions passed. Pivoted to lopdf;
#     no PDFium content API, no reload_in_place, no crash.

npm run check    # tsc ✓  eslint src ✓  clippy --all-targets -D warnings ✓
npm run test     # 145/145 (page-sizes: 4 new)
npm run test:rust
#   resize.rs: 5 (a4 / all-pages / undo-restores / rejects-nonpositive /
#   out-of-range) + 1 ignored artifact. cos.rs: +1
#   (cos_resizes_sets_mediabox_and_wraps_content — MediaBox=A4, first content
#   stream is the `q … cm` wrapper, reopens in PDFium). Full suite green, NO
#   SIGSEGV (the whole point of the pivot).

# PDF write-path artifact (ignored, on demand):
cargo test --test resize resize_writes_verification_artifact \
  -- --ignored --test-threads=1   # (from src-tauri/)
#   → /tmp/vibepdf-verify-resized.pdf (hello.pdf Letter → A4, preserve-aspect);
#     copied to Sample PDFs/. MediaBox confirmed [0 0 595.28 841.89].
```

Mechanism + rationale recorded in `docs/04` "Structural edits via lopdf" and
`Learning.md`. Limits (annotations not re-scaled; CropBox dropped; no
orientation match) → `BACKLOG.md`. Left `[~]` pending the human cross-reader of
`Sample PDFs/vibepdf-verify-resized.pdf` + the in-app flow.

---

### P3.A1 — annotation tool framework (this commit) — start of Phase 3

```bash
# No new deps. Frontend-only infrastructure: the annotation tool framework
# (contract + pure lifecycle state machine + screen↔PDF coords + registry) and
# two zustand stores (tool-store, annotation-store). No Rust, no IPC — annotations
# don't touch PDF bytes until P3.B1.

# Verification (frontend only — nothing Rust changed, so clippy/test:rust unaffected):
npx tsc --noEmit                 # ✓
npx eslint src --max-warnings=0  # ✓
npx vitest run                   # 160/160 (was 146): +lifecycle 5, +coords 4,
                                 #   +annotation-store 5. coords has round-trip
                                 #   property tests for all 4 page rotations.
```

Realizes the `useToolStore` + `§Edit tools` contract that `docs/04` described but
hadn't built; store table in `docs/04` synced to reality + the two new stores.
Pure infra, **no user-visible demo** — first exercised end-to-end by A2 (render
layer) + B1 (highlight). Left `[~]`.

---

### P3.A2 — annotation render layer (this commit)

```bash
# No new deps. Frontend-only: the per-page SVG annotation overlay
# (annotation-layer.tsx) + a PageSlot restructure to host it + a temporary
# toolbar toggle to make it visible. No Rust, no IPC (persistence is B1).

# Added a jsdom PointerEvent polyfill to src/test-setup.ts — jsdom doesn't
# implement PointerEvent, so fireEvent.pointerDown produced events with no
# clientX (committed rect came out NaN until the polyfill).

npx tsc --noEmit                 # ✓
npx eslint src --max-warnings=0  # ✓
npx vitest run                   # 164/164 (+annotation-layer 4): render a
                                 #   committed rect at the mapped position,
                                 #   render the draft, commit from down→move→up
                                 #   with the rect tool, select on click when idle.
npm run check                    # tsc + eslint + clippy (no Rust changed) ✓
```

Mounting detail (docs/04 + Learning.md): the canvas mounts into an inner div so
the React overlay (sibling) survives PageSlot's imperative child-clear; the outer
flow element stays registered for scroll (keeps the offsetTop jump-to-page fix).
Temporary "▭" toggle + example-tool registration = A2 demo scaffolding (removed
in B1/C1). Left `[~]` pending the human's in-app preview-rect check.

---

### P3.B1a — text selection + markup preview (this commit)

```bash
# No new deps (pdfjs-dist already present — using its v5 TextLayer). Frontend
# only: a PDF.js text layer (selectable text), selection→/QuadPoints math, a
# markup toolbar, and overlay rendering of markup. Preview-only — NO PDF write,
# NO IPC (persistence is B1b).

npx tsc --noEmit                 # ✓ (DistributiveOmit fix for the Annotation union)
npx eslint src --max-warnings=0  # ✓
npx vitest run                   # 173/173 (+9): quads 4, apply-markup 3,
                                 #   annotation-layer markup +2.
npm run check                    # tsc + eslint + clippy (no Rust changed) ✓
```

Removed the temporary A2 "▭" toggle from ZoomToolbar (replaced by MarkupToolbar).
Text layer styling = a minimal `.textLayer` port in styles/globals.css. Markup is
selection-driven (not the A1 stepTool lifecycle). Left `[~]` pending the in-app
select-text → highlight preview check.

---

### P3.B1b — persist text markup to the PDF (this commit)

```bash
# No new deps. The first annotation WRITE path: cos::add_text_markup (lopdf)
# builds the annotation dict (/Subtype, /QuadPoints, /C) + a generated /AP
# appearance (Multiply-blended quads / lines), append to /Annots. PDFium can't
# author a coloured annotation, so it's all lopdf; PDFium preserves it on save.
# TextMarkupEdit (annotation.rs) + AddTextMarkup actor msg + pdf_add_text_markup
# command. MarkupToolbar now writes via IPC (not the store) → epoch reload → the
# PDF.js canvas renders the /AP.

npm run check          # tsc + eslint src + clippy ✓
#   clippy fixes: backtick PDFium/BBox (doc_markdown); extracted
#   markup_appearance_content to get add_text_markup under 100 lines.
npm run test           # 173/173 (markup is Rust-tested; FE unchanged count)
npm run test:rust      # EXIT 0, no SIGSEGV. text_markup.rs: 3 (highlight persists
#   with /AP through save / undo removes it / rejects empty quads) + 1 ignored
#   artifact. cos.rs: +3 (writes annot+/AP / maps each subtype / rejects bad input).

# PDF write-path artifact (ignored, on demand):
cargo test --test text_markup markup_writes_verification_artifact \
  -- --ignored --test-threads=1     # (from src-tauri/)
#   → /tmp/vibepdf-verify-highlight.pdf (hello.pdf + a highlight); copied to
#     Sample PDFs/. strings confirms /Subtype/Highlight + /QuadPoints + /AP.
```

Rendering decision: the **PDF.js canvas** renders committed markup from `/AP`
(consistent with every other edit). Left `[~]` pending the human cross-reader
(Acrobat/Preview/Chrome) + the in-app main-view render check.

---

### P3.B1 (debug + Clear) — text layer working in WKWebView (this commit)

```bash
# No new deps. Fixing why text selection (B1a) didn't work in the real macOS
# webview, then a Clear-markup button. All issues were WKWebView-specific and
# invisible in tests — found via the dev console (captured in the vite log):
#   1. ReadableStream has no async iterator → getTextContent threw. Polyfill in
#      src/polyfills.ts (imported first in main.tsx).
#   2. Hand-rolled .textLayer CSS missing the span font-size/scaleX rules → spans
#      had no size, a click selected the whole page. Ported the full v5 CSS.
#   3. CSS round() maybe unsupported → pin explicit px layer size after render.
#   4. getDocument needs standardFontDataUrl/cMapUrl (served from /pdfjs/, copied
#      by scripts/copy-pdfjs-worker.mjs, now also copies standard_fonts + cmaps).
#   5. getClientRects() returns the container's full-page rect → filtered in
#      apply-markup (was highlighting the whole page + stacking).

# Clear markup: cos::clear_text_markup strips all Highlight/Underline/StrikeOut/
# Squiggly from /Annots (prune_objects GCs them); ClearMarkupEdit +
# pdf_clear_text_markup + a red "Clear" button (one undoable edit).

npm run check     # tsc + eslint src + clippy ✓
npm run test      # 173/173
npm run test:rust # EXIT 0. cos.rs +1 (cos_clears_text_markup). All green.

node scripts/copy-pdfjs-worker.mjs   # worker + 16 standard fonts + 169 cmaps → public/pdfjs/
```

Human-verified in-app: select text → Highlight lands on it (PDF.js renders the
`/AP`), ⌘Z removes, Clear wipes all markup. Three WKWebView fixes recorded in
`docs/04` "WebView quirks" + `Learning.md`. Flipped A2 / B1a / B1b → `[x]`.

---

### P3.B2a — sticky notes: place / edit / delete + persist (this commit)

```bash
# No new deps. Second annotation type, first INTERACTIVE one. Backend is lopdf
# (PDFium can't author a coloured annotation):
#   cos::add_text_note   → /Text dict: /Contents /T /NM /M /CreationDate
#                          /Name /Note /C [1 0.82 0] /F 28 /Open false, NO /AP.
#   cos::update_text_note / delete_annotation  → find by /NM, edit or remove.
#   helpers: pdf_date_now (D:YYYYMMDDHHmmSSZ, no chrono), append_annotation,
#            find_annotation_by_nm.
# annotation.rs: shared cos_edit() + AddNote/UpdateNote/DeleteAnnotation edits
# (byte-handoff, inverse = RestoreDocEdit). Actor msgs + 3 pdf_* commands.
# Frontend: notes have no /AP, so they are OVERLAY-rendered by a new HTML
# NoteLayer (sibling of the SVG AnnotationLayer, on top) from the store — NOT the
# canvas. note-tool.ts (click-to-place), NotePopup.tsx (body + Save + Delete),
# a "Note" toggle in MarkupToolbar. Placement persists immediately (store id =
# the /NM); update/delete are their own undoable edits.

npm run check          # tsc + eslint src + clippy ✓
#   eslint.config.js: added HTMLTextAreaElement to the DOM-globals allowlist.
#   TS: narrowed example-rect-tool + annotation-layer for the new "note" union
#   member (filter out notes from the SVG layer).
npm run test           # 185/185 (+12: notes IPC 4, note-tool 3, NoteLayer 5)
npm run test:rust      # EXIT 0. text_note.rs: 4 (persists w/ author+NM / update /
#   delete / undo) + 1 ignored artifact. cos.rs: +2 (note dict shape: /Text /Note
#   /F 28 + NO /AP; update+delete by /NM).

# PDF write-path artifact (ignored, on demand):
cargo test --test text_note note_writes_verification_artifact -- --ignored
#   → Sample PDFs/vibepdf-verify-note.pdf (hello.pdf + a note); keeps
#     /Subtype/Text + /Contents + /NM through the PDFium save.
```

Rendering decision: notes carry **no `/AP`**, so the HTML `NoteLayer` overlay
draws the icon + popup from the store (vs markup, which the canvas paints from
`/AP`). Left `[~]` pending the human in-app ritual (place → type → Save → reopen
→ edit → Delete → ⌘Z → ⌘S) + cross-reader (Acrobat/Preview/Okular). Reopening a
saved file does not yet repopulate notes in-app — that's **B2b**.

---

### P3.B2b — notes as a PDF projection: re-open + undo-safe (this commit)

```bash
# No new deps. NO new write path — a READ path that makes the note overlay a
# projection of the PDF (was an in-session cache). Fixes two B2a gaps: reopened
# files showed no notes in-app, and actor ⌘Z left a ghost icon.
#   cos::read_text_notes(bytes) -> Vec<NoteData{nm,page,x,y,content,author}>
#     — inverse of add_text_note; walks /Annots, keeps /Subtype /Text.
#   actor ReadNotes (READ-ONLY, like GetBytes: serialize under PDFium lock →
#     lopdf-parse; no edit, no history). command pdf_read_text_notes.
# Frontend: useNotesSync(documentId) reads + replaceNotes (new store action that
#   swaps only note-type annots) keyed on [documentId, edit-epoch] — fires on
#   open/restore/tab AND every reload-edit (undo/redo bump the epoch). Mounted in
#   App.tsx beside useHistory. Placement does NOT bump the epoch (keeps the
#   optimistic icon). Notes without /NM get a synthesized obj-<n>-<g> id.

npm run check          # tsc + eslint src + clippy ✓
#   clippy: backtick PDFDocEncoding (doc_markdown); allow cast_precision_loss on
#   rect_lower_left (i64 Rect int → f32; coords are MediaBox-bounded).
npm run test           # 191/191 (+6: readTextNotes 1, replaceNotes 1, useNotesSync 4)
npm run test:rust      # EXIT 0. text_note.rs +4 (read-back-after-reopen / update+
#   delete reflected / empty on plain / undo+redo tracked). cos.rs +2 (reads notes
#   in page order w/ correct fields / empty on plain pdf).
```

No new write-path artifact (read-only step). The human ritual reuses the B2a
artifact `Sample PDFs/vibepdf-verify-note.pdf`: reopen it in VibePDF → the note
icon + body return. Left `[~]` pending that + the ⌘Z-hides / ⌘⇧Z-restores check.

---

### P3.B2a (fix) — note popup was click-through (this commit)

```bash
# In-app verification of B2a surfaced a real bug: a newly-placed note auto-
# focused (programmatic) but you couldn't click into the textarea or hit Save —
# clicks fell through to the text layer and stole focus. Cause: after placement
# we drop the tool to idle, which sets the NoteLayer container to
# `pointer-events: none`; the popup is a child and inherited it. The icons set
# `pointer-events: auto` but the popup didn't. Fix: NotePopup root opts back in
# with `pointerEvents: "auto"`. Regression test added to note-layer.test.tsx.

npm run check   # tsc + eslint src + clippy ✓
npm run test    # note-layer 5/5 (incl. the new pointer-events assertion); 191 total
```

Human-verified in-app: place note → click in → type → Save works; edit + delete
of an existing note (both click inside the popup) work too.

---

### P3.B3a — free-text boxes, uniform style (this commit)

```bash
# No new deps. Third annotation type — typed text drawn via a generated /AP
# (like markup, canvas-rendered):
#   cos::add_free_text → /Subtype /FreeText + /Rect /Contents /DA /F 4 /P + an /AP
#     form XObject whose stream draws the text (BT /F1 <size> Tf … TL <x> <y> Td
#     (line) Tj T* … ET) with a self-contained base-14 /Font (no AcroForm /DR
#     needed). Helpers base_font (12 base-14 names) + pdf_escape (\ ( ) ).
#   FreeTextEdit (annotation.rs, via cos_edit) + AddFreeText actor msg +
#     pdf_add_free_text command.
# Frontend: ToolOptions += fontFamily/fontSize/bold/italic. FreeTextLayer (new
#   HTML overlay) = drag-to-box + a transient <textarea> editor; on Add → IPC →
#   bumpEpoch → the CANVAS renders the /AP (overlay holds no committed boxes).
#   "Text" toggle + font/size/B/I controls in MarkupToolbar.

npm run check          # tsc + eslint src + clippy ✓
#   clippy: renamed locals in free_text_appearance_content (many_single_char_names).
#   TS: updated the two ToolOptions test fixtures (lifecycle, note-tool) for the
#   new fields; dropped a stale jsx-a11y eslint-disable (rule not configured).
npm run test           # 201/201 (+10: addFreeText 2, free-text helpers 4, FreeTextLayer 4)
npm run test:rust      # EXIT 0. free_text.rs: 3 (persists through save / undo removes
#   / rejects empty rect) + 1 ignored artifact. cos.rs: +4 (writes annot+/AP / font
#   variants / escapes+splits lines / rejects empty rect + bad font).

# PDF write-path artifact (ignored, on demand):
cargo test --test free_text free_text_writes_verification_artifact -- --ignored
#   → Sample PDFs/vibepdf-verify-freetext.pdf (hello.pdf + a Times-Bold red box);
#     keeps /Subtype/FreeText + /AP through the PDFium save.
```

Rendering decision: the committed box has an `/AP`, so the **PDF.js canvas** draws
it (consistent with markup). Left `[~]` pending the human in-app check that the
canvas renders the FreeText `/AP` (the key unknown — overlay fallback ready) +
cross-reader (Acrobat/Preview/Okular). Rich text / underline / re-edit are **B3b**.

---

### P3.D1 — annotation sidebar (read-only list) (this commit)

```bash
# No new deps. First READ-over-all-kinds: the inverse projection that lets you
# see/search/filter/jump-to every annotation.
#   cos::read_annotations → Vec<AnnotationInfo{id (lopdf obj id), page, kind,
#     rect, contents, author, modified}>; whitelists 6 subtypes, parse_pdf_date
#     for /M (inverse of pdf_date_now). actor ReadAnnotations (read-only, like
#     ReadNotes). command pdf_read_annotations.
# Frontend: AnnotationPanel (re-reads on [documentId, epoch]); pure
#   annotation-filter (search + type/author/date + group-by-page); view-store
#   showAnnotations + a toolbar toggle; annotation-selection-store +
#   SelectionHighlightLayer (dashed box from the read /Rect); click → scrollToPage.

npm run check          # tsc + eslint src + clippy ✓
#   eslint: memoized `all = list ?? []` (exhaustive-deps; array identity churned
#   the useMemos).
npm run test           # 217/217 (+16: annotations IPC 3, annotation-filter 8, panel 5)
npm run test:rust      # EXIT 0. cos.rs +2 (reads all kinds w/ fields+date / skips
#   links + empty on plain). read_annotations.rs: 3 (reads all / reflects undo /
#   empty) + 1 ignored demo artifact.

# Sidebar demo artifact (ignored, on demand):
cargo test --manifest-path src-tauri/Cargo.toml --test read_annotations \
  writes_sidebar_demo_artifact -- --ignored
#   → Sample PDFs/vibepdf-verify-annots.pdf (a highlight + a note + a free-text).
```

Read-only step (no write path). Left `[~]` pending the human in-app pass (add the
three kinds → toggle Annotations → grouped list, search + filters narrow, click →
scroll + dashed highlight). Per-annotation delete/edit + author/date on markup/
free-text are deferred (BACKLOG) — they need durable `/NM` handles.

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
