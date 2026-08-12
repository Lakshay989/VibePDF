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

### P3.C1a — shapes: rectangle + ellipse (this commit)

```bash
# No new deps. Drag-to-size shapes, canvas-rendered via a generated /AP. Also the
# A2→C1 step: the annotation-layer commit now PERSISTS (was store-only).
#   cos::add_shape → /Square (rect) | /Circle (ellipse): /Rect /C (+/IC fill) /CA
#     /BS/W + /AP (re…S/B for rect; 4-Bézier kappa ellipse), inset by ½ stroke.
#   ShapeEdit (annotation.rs) + AddShape actor msg + pdf_add_shape command.
# Frontend: ToolOptions += fillColor (string|null). shape-tools.ts
#   (makeDragRectTool → rectangleTool/ellipseTool). annotation-layer registers
#   them at module load + commit → addShape IPC → bumpEpoch (NOT the store).
#   Rectangle/Ellipse toggles + a fill control in MarkupToolbar.

npm run check          # tsc + eslint src + clippy ✓
#   clippy: named the ellipse coords (cx/cy/rx/ry/kx/ky) to dodge
#   many_single_char_names; buffer `out` not `c`.
#   TS: added fillColor:null to 3 ToolOptions test fixtures.
npm run test           # 226/226 (+9: addShape 3, shape-tools 6; annotation-layer
#   commit test rewritten to assert the IPC persist, not a store add)
npm run test:rust      # EXIT 0. shapes.rs: 3 (persists through save / undo removes
#   / rejects empty rect) + 1 ignored artifact. cos.rs: +3 (Square+Circle w/ /AP /
#   fill sets /IC + unfilled omits it / rejects bad kind+empty rect).

# PDF write-path artifact (ignored, on demand):
cargo test --manifest-path src-tauri/Cargo.toml --test shapes \
  shape_writes_verification_artifact -- --ignored
#   → Sample PDFs/vibepdf-verify-shapes.pdf (a filled rect + a filled ellipse).
```

Rendering decision: shapes have an `/AP`, so the **canvas** draws them (like
markup/free-text). Left `[~]` pending the human in-app canvas-render check +
cross-reader. Line/arrow/polygon are **C1b**; select/delete is the D1 follow-up.

---

### P3.D1d — select + delete annotations (P3-ANN-012) (this commit)

```bash
# No new deps, no new IPC. From the 2026-06-18 sweep: couldn't remove a committed
# free-text/shape. Keystone = a stable identity:
#   cos::add_text_markup / add_free_text / add_shape now stamp /NM = uuid (notes
#     already had one). read_annotations returns /NM as the handle (else
#     obj:<num> <gen>). The EXISTING cos::delete_annotation deletes by /NM, now
#     also by obj:<...> (parse_object_id). annotation_kind += Square/Circle, so
#     shapes finally show in the sidebar (D1 predated C1a — they were invisible).
# Frontend: AnnotationKind += rectangle/ellipse; AnnotationPanel row ✕ + Delete-
#   key (focus-guarded) → deleteAnnotation (reused) → setHistory + bumpEpoch →
#   canvas reload drops the /AP, list + note overlay re-sync via the epoch.
# Spec: added P3-ANN-012 to docs/02 (human-approved).

npm run check          # tsc + eslint src + clippy ✓
npm run test           # 228/228 (+2 AnnotationPanel: row-delete + Delete-key)
npm run test:rust      # EXIT 0. cos.rs +4 (annotations carry /NM + delete by it /
#   delete by object-id fallback / [shapes now surfaced via the kinds map]).
#   annotation_delete.rs: 2 (delete each kind through the actor / delete undoable).
```

A test caught a latent bug: `annotation_kind` (written in D1, before C1a) never
mapped `/Square`/`/Circle`, so shapes were absent from the sidebar — fixed here.
Left `[~]` pending the human in-app pass (shapes now listed; select → ✕/Delete
removes from page + list; ⌘Z; ⌘S). Edit-a-committed-shape + in-canvas select are
still deferred (BACKLOG).

---

### P3.B3a (fix) — free-text no longer clipped by a large font (this commit)

```bash
# Sweep finding #3: a font taller than the dragged box was cut off, because the
# /AP form clips to BBox == Rect. Fix in cos::add_free_text: grow the box DOWN
# (top edge fixed) to fit line_count × leading + descender padding. The editor
# <textarea> also grows to >= ~1.4 line-heights so a big font is visible while
# typing. Width/no-wrap still a documented limit (B3b).

npm run check     # tsc + eslint src + clippy ✓
npm run test      # 228/228 (free-text-layer 4 still green)
npm run test:rust # cos.rs +1 (grows_box_to_fit_large_font: 48pt in a 20pt box →
#   /Rect height ≥ 110pt, top fixed at 700).
```

---

### P3.D1e — edit a free-text box in place (P3-ANN-013) (this commit)

```bash
# No new deps. The "update" half of the sweep (#2): re-edit a committed free-text.
#   cos::read_free_text(nm) → parse text + style back (/Contents, /Rect; size+colour
#     from /DA; family+bold/italic from the /AP /BaseFont — inverse of base_font).
#   cos::update_free_text(nm, …) → rewrite /Contents + /Rect (grow-to-fit) + /DA +
#     /AP IN PLACE, preserving /NM; old /AP GC'd by prune_objects. add_free_text
#     refactored to share free_text_appearance + grow_free_text_rect.
#   UpdateFreeTextEdit + read-only ReadFreeText + UpdateFreeText actor msgs +
#     pdf_read_free_text / pdf_update_free_text.
# Frontend: annotation-edit-store (sidebar → FreeTextLayer request channel). The
#   sidebar ✎ reads the box + posts a request; the page's FreeTextLayer claims it,
#   opens the editor pre-filled (sets the toolbar to the box's style), commits via
#   updateFreeText (the editor grew one field, editNm; commit branches on it).
# Spec: added P3-ANN-013 to docs/02 (human-approved).

npm run check          # tsc + eslint src + clippy ✓
#   clippy: merged font_from_base Helvetica arm into the wildcard (match_same_arms);
#   allow cast on rgb_to_hex.
npm run test           # 232/232 (+4: freetext IPC 2, free-text-layer edit 1,
#   AnnotationPanel edit 1)
npm run test:rust      # EXIT 0. cos.rs +3 (DA/BaseFont style round-trip / update
#   keeps /NM / rejects unknown nm). free_text_edit.rs: 2 (edit round-trips + undo
#   restores / read None for a non-free-text handle).
```

Update-in-place (not delete+re-add) so the `/NM` — and the sidebar
selection/identity — survives the edit. Shape style re-edit + in-canvas
double-click are deferred (BACKLOG). Left `[~]` pending the human in-app pass.

---

### P3.C1b₁ — line + arrow annotations (this commit)

```bash
# No new deps. First POINTS-based shape (C1a was bbox-based). C1b split by gesture:
# line/arrow = drag (now); polygon = multi-click (C1b₂, deferred).
#   cos::add_line → /Line dict (/L [x1 y1 x2 y2], /C, /CA, /BS/W, /NM, + /LE
#     [/None /OpenArrow] for the arrow) + a generated /AP stroking the segment +
#     an arrowhead V (arrowhead_points aims it back along the segment); /BBox
#     padded for the head + stroke width. annotation_kind += /Line → "line".
#   LineEdit + AddLine actor msg + pdf_add_line.
# Frontend: LineAnnotation draft; line-tools.ts (makeLineTool → line/arrow drag
#   reducers). annotation-layer registers them, renders the LineShape draft (SVG
#   line + arrowhead), commits a `line` draft via addLine → bumpEpoch (canvas).
#   AnnotationKind += "line"; Line + Arrow toolbar toggles.

npm run check          # tsc + eslint src + clippy ✓
#   TS: guard the nullable onPointerDown return in line-tools.test before narrowing.
npm run test           # 238/238 (+6: line-tools 3, lines IPC 2, annotation-layer line 1)
npm run test:rust      # EXIT 0. cos.rs +4 (line+/L+/AP / arrow sets /LE + 2nd
#   stroke / listed+deletable / rejects zero-length). lines.rs: 2 (line+arrow
#   persist through save / undo removes) + 1 ignored artifact.

# PDF write-path artifact (ignored, on demand):
cargo test --manifest-path src-tauri/Cargo.toml --test lines \
  line_writes_verification_artifact -- --ignored
#   → Sample PDFs/vibepdf-verify-lines.pdf (a line + an arrow).
```

Lines are canvas-rendered from the `/AP` (like the other shapes); the overlay
draws only the live drag preview. Polygon (multi-click) is **C1b₂**; arrowhead
style options + line geometry edit are deferred. Left `[~]` pending the human
in-app + cross-reader pass.

---

### P3.C1b₂ — polygon (multi-click) (this commit)

```bash
# No new deps. The first NON-drag tool. Completes the C1 shapes track.
#   cos::add_polygon → /Polygon (closed, fillable) | /PolyLine (open) via /Vertices
#     + /C (+/IC fill) /CA /BS/W /NM + a generated /AP (m→l…, h to close, B/f/S
#     paint); /BBox padded for the stroke width. annotation_kind += /Polygon →
#     "polygon", /PolyLine → "polyline".
#   PolygonEdit + AddPolygon actor msg + pdf_add_polygon (points: Vec<[f32;2]>).
# Frontend: NO framework change — multi-click doesn't fit the drag lifecycle, so a
#   self-contained PolygonLayer (like Note/FreeText layers) owns the gesture:
#   click adds a vertex (deduping the dbl-click's 2nd down), move = rubber-band,
#   dbl-click/Enter finishes → addPolygon → bumpEpoch, Esc cancels. Vertices stored
#   in PDF points. Polygon toolbar toggle + fill control; AnnotationKind +=
#   polygon/polyline. Spec: polygon matches P3-ANN-004; POLYLINE not exposed in UI
#   (spec says "polygons"; the cos `closed` flag is built+tested for later).

npm run check          # tsc + eslint src + clippy ✓
#   clippy: slice-pattern the first vertex (index_refutable_slice).
#   eslint: ReactPointerEvent<Element> not SVGSVGElement (not in the globals list).
npm run test           # 245/245 (+7: polygons IPC 2, polygon-layer 5)
npm run test:rust      # EXIT 0. cos.rs +4 (polygon /Vertices+/AP+h+fill / polyline
#   open+unfilled / listed+deletable / rejects <3|<2). polygons.rs: 3 (polygon+
#   polyline persist / undo removes / rejects too few) + 1 ignored artifact.

# PDF write-path artifact (ignored, on demand):
cargo test --manifest-path src-tauri/Cargo.toml --test polygons \
  polygon_writes_verification_artifact -- --ignored
#   → Sample PDFs/vibepdf-verify-polygon.pdf (a filled 5-gon).
```

Self-contained overlay over a framework change (the "rule of three" — no shared
multi-click lifecycle until a third such tool appears). Left `[~]` pending the
human in-app + cross-reader pass. Polyline UI + vertex editing deferred (BACKLOG).

---

### P3.C2 — freehand ink with smoothing (this commit)

```bash
# No new deps. The pen tool. Smoothing lives in the FRONTEND, the /AP is a ribbon.
#   src/tools/ink/ink.ts: simplify (drop <1pt jitter) → catmullRomResample
#     (interpolating Catmull-Rom spline, even ~3pt spacing; pressure LINEARLY
#     interpolated so it can't overshoot [0,1]). Pure + DOM-free.
#   cos::add_ink → /Ink (/InkList one sub-path, /C /CA /BS/W /NM) + a generated /AP
#     that's a VARIABLE-WIDTH FILLED RIBBON: centreline ±ink_half_width(pressure)
#     along each averaged normal, filled non-zero (f). /BBox padded by the MAX
#     half-width (a hard press exceeds base width → would clip otherwise).
#     annotation_kind += /Ink → "ink". InkEdit + AddInk + pdf_add_ink
#     (points: Vec<[f32;3]> = [x,y,pressure]).
# Frontend: NO framework change — a drag, but it needs the WHOLE path + per-sample
#   pressure (stepTool is start+end only), so a self-contained InkLayer (like
#   PolygonLayer) owns pointer-capture, dedups sub-px samples, previews the raw
#   path, smooths on release → addInk → bumpEpoch. Pen toolbar toggle;
#   AnnotationKind += ink. Spec: P3-ANN-005 (pressure "where available"; a
#   mouse/trackpad reports a constant 0.5 → uniform width).

npm run check          # tsc + eslint src + clippy ✓ (clean first pass)
npm run test           # 259/259 (+14: smoothing 9, ink IPC 2, ink-layer 3)
npm run test:rust      # EXIT 0. cos.rs +4 (ink /InkList+/AP ribbon / pressure
#   widens the Rect / listed+deletable / rejects a tap|coincident). ink.rs: 3
#   (persist / undo removes / rejects a tap) + 1 ignored artifact.
# A real bug a test caught: padding /BBox by `width` (as line/polygon do) clipped a
#   full-pressure ribbon; pad must be the max half-width in the stroke.

# PDF write-path artifact (ignored, on demand):
cargo test --manifest-path src-tauri/Cargo.toml --test ink \
  ink_writes_verification_artifact -- --ignored
#   → Sample PDFs/vibepdf-verify-ink.pdf (a pressure-modulated sine wave).
```

Frontend smoothing + a fill-based ribbon (renders everywhere; uniform pressure =
constant width). Left `[~]` pending the human in-app + cross-reader pass.
Multi-stroke /InkList grouping, eraser, and a Bézier /AP deferred (BACKLOG).

---

### P3.C2 — in-app verification fixes (four commits)

```bash
# No deps / scripts. Four UI bugs the human found verifying C2; tests-only repro
# where possible. Each its own focused commit.
#  1. Text selection while drawing: PDF.js text spans carry z-index:1 → paint over
#     the overlay → I-beam + native selection mid-draw. Drawing tools now set the
#     text layer pointer-events:none (via className, not inline style).
#  2. Pages sidebar duplicated: ThumbnailPanel + AnnotationPanel are siblings both
#     keyed `documentId` → React duplicate-key → ghost columns. Distinct keys.
#     (Console "two children with the same key" was the tell — not HMR.)
#  3. Date filter: `new Date("YYYY-MM-DD")` parses UTC → hid today's annotations in
#     +UTC zones. Parse local (`dateInputToMs`) + controlled input + ✕ clear.
#  4. Polygon: click the first vertex to close (+ highlight it); abandon on tool
#     switch so the rubber-band doesn't linger.
#
# REVERTED (do not ship blind): a canvas double-buffer + doc-swap to kill the
# per-edit "refresh flash". Sound in principle but skewed page geometry (shapes
# off-spot, ovals→circles) and I can't iterate on the pixels without computer-use.
# Restored the known-good render path; flash deferred.

npm run check          # tsc + eslint src + clippy ✓
npm run test           # 267/267 (+6: text-layer-gating 2, date helpers 4 [polygon +2])
```

Bug fixes during C2 verification (not a new roadmap step). C2 stays `[~]`; the
refresh flash is the one open item.

---

### P3.C3a — stamp library + custom text stamps (this commit)

```bash
# No new deps. /Stamp annotations with a generated /AP. Image stamps split to C3b.
#   cos::add_stamp → /Stamp (/Name sanitized, /Contents, /C, /CA, /F 4, /P, /NM) +
#     a generated /AP: ExtGState opacity + bold base-14 /F1; a stroked border
#     (re S) and the bold UPPERCASE label centred (font size auto-fit to the box
#     via a 0.62-em Helvetica-Bold advance estimate). annotation_kind += Stamp.
#   StampEdit + AddStamp actor msg + pdf_add_stamp (rect:[f32;4], text, name, ...).
# Frontend: NO framework change — click-to-place doesn't fit the drag lifecycle, so
#   a self-contained StampLayer (like Note/Polygon/Ink) drops the armed stamp on a
#   click. tools/stamp/stamps.ts (built-in library + stampRectAt), StampPalette
#   (pick/type → arm in a stamp-store), Stamp toolbar toggle (disarms on tool
#   change). AnnotationKind += stamp.

npm run check          # tsc + eslint src + clippy ✓
#   clippy pedantic: hoisted the 0.62 glyph-em to a module const
#   (items_after_statements) + renamed box w/h → bw/bh (many_single_char_names).
npm run test           # 275/275 (+8: stamps lib 4, stamp-layer 3, stamps IPC 1)
npm run test:rust      # EXIT 0. cos.rs +4 (subtype/name/AP, sanitize+uppercase,
#   listed+deletable, rejects empty text|rect). stamp.rs: 3 (persist / undo
#   removes / rejects empty) + 1 ignored artifact.

# PDF write-path artifact (ignored, on demand):
cargo test --manifest-path src-tauri/Cargo.toml --test stamp \
  stamp_writes_verification_artifact -- --ignored
#   → Sample PDFs/vibepdf-verify-stamp.pdf (an APPROVED + a CONFIDENTIAL stamp).
```

The library + text half of P3-ANN-006. Left `[~]` pending the human in-app +
cross-reader pass. Image stamps are C3b (image XObject embedding).

---

### P3.C4a — measurement tools + calibration (this commit)

```bash
# No new deps. Distance/perimeter/area measurements + draw-to-calibrate. The PDF
# /Measure dict (Acrobat live re-measure) is split to C4b.
#   cos::add_measure → /Line|/PolyLine|/Polygon with a dimension /IT
#     (LineDimension/PolyLineDimension/PolygonDimension), /Contents=value, /C,
#     /CA, /BS/W, /NM + a generated /AP (geometry stroke + the bold value label
#     centred on the centroid). read_annotations: a measurement /IT reads back as
#     "measure" (not the bare shape). MeasureEdit + AddMeasure + pdf_add_measure.
# Frontend: measure MATHS are pure (tools/measure/measure.ts — calibrationScale,
#   straightDistance/pathLength, polygonArea [shoelace, abs], measureValue [area
#   scales by scale²], formatMeasurement). A self-contained MeasureLayer reuses
#   the polygon multi-click; a `calibrating` store flag switches it between
#   "stash reference length → CalibrateDialog" and "persist measurement"; distance
#   auto-finishes at 2 clicks. Measure toolbar toggle + MeasureControls.
#   AnnotationKind += measure.

npm run check          # tsc + eslint src + clippy ✓
#   clippy pedantic: unnested_or_patterns → Some(b"A" | b"B" | b"C").
#   eslint: dropped a disable for an unconfigured jsx-a11y rule.
npm run test           # 289/289 (+14: calibration 9, measure-layer 4, measure IPC 1)
npm run test:rust      # EXIT 0. cos.rs +4 (distance Line+IT+label, area Polygon
#   dimension, reads back as "measure"+deletable, rejects bad kind|empty|<min).
#   measure.rs: 3 (persist 2 / undo removes / rejects bad kind) + 1 ignored.

# PDF write-path artifact (ignored, on demand):
cargo test --manifest-path src-tauri/Cargo.toml --test measure \
  measure_writes_verification_artifact -- --ignored
#   → Sample PDFs/vibepdf-verify-measure.pdf (a distance + an area).
```

The three tools + calibration + displayed value (the spec's user-facing half).
Left `[~]` pending the human in-app + cross-reader pass. The /Measure dict (live
re-measure) + persisted calibration are C4b.

---

### P3.D2 — reply threads (this commit)

```bash
# No new deps. A reply is a /Text linked to its parent via /IRT (per the spec).
#   cos::add_reply(parent_handle, author, content) → a /Text with /IRT (ref to
#     parent) + /RT /R + /Contents/T/M/NM, inheriting the parent's page + /Rect.
#     No /AP (lives in the thread). read_annotations resolves /IRT → parent handle
#     (AnnotationInfo.in_reply_to); read_text_notes SKIPS /IRT (no stray page
#     icon). Shared resolve_handle with delete (same /NM-or-obj: id). ReplyEdit +
#     AddReply + pdf_add_reply.
# Frontend: buildThreads (annotation-filter, pure) walks /IRT to a thread root +
#   nests replies flat/chronological; orphan + cycle safe. AnnotationPanel renders
#   root + nested replies + an inline Reply composer (the spec's right-click → a
#   discoverable button instead; menu deferred). AnnotationInfo += inReplyTo.

npm run check          # tsc + eslint src + clippy ✓ (clean)
#   (existing AnnotationInfo literals in tests gained inReplyTo: null)
npm run test           # 296/296 (+7: buildThreads 5, replies IPC 1, panel thread 1)
npm run test:rust      # EXIT 0. cos.rs +4 (links via /IRT + surfaces inReplyTo,
#   not read as a page note, reply-to-any-kind + deletable, rejects unknown
#   parent). reply_thread.rs: 3 (persist+link / undo removes only the reply /
#   rejects unknown parent) + 1 ignored artifact.

# PDF write-path artifact (ignored, on demand):
cargo test --manifest-path src-tauri/Cargo.toml --test reply_thread \
  reply_writes_verification_artifact -- --ignored
#   → Sample PDFs/vibepdf-verify-reply.pdf (a note + two replies).
```

Reply threads via /IRT, threaded in the sidebar. Left `[~]` pending the human
in-app + cross-reader pass. Reply editing + /State (Accepted/Rejected) + a
right-click menu are deferred (BACKLOG).

---

### P3.E1 — XFDF import / export (this commit)

No new dependencies (hand-rolled XML parser; that was the whole point — see the
`/plan` decision). No `cargo add` / `npm install`.

```bash
# Verification gates
npm run check          # tsc + eslint(0 warn) + cargo clippy --all-targets -D warnings → clean
npm run test           # 300/300 (+4: interchange IPC 2, panel export/import 2)
npm run test:rust      # EXIT 0. xfdf.rs units: 8 (escape, entity decode, float-parse
#   leniency, collect-known-only + skip unknown, prolog/comment/self-close, malformed
#   no-panic, subtype map, DA parse). xfdf_roundtrip.rs: 4 (export covers every
#   subtype, geometry+contents survive a fresh import, reply thread survives,
#   import is one undoable edit) + 1 ignored artifact.

# PDF write-path artifact (ignored, on demand) → Sample PDFs/ + /tmp:
cargo test --manifest-path src-tauri/Cargo.toml --test xfdf_roundtrip \
  xfdf_writes_verification_artifact -- --ignored
#   → Sample PDFs/vibepdf-verify-xfdf.pdf (7 annotations) + .xfdf (the export).
cp "Sample PDFs/vibepdf-verify-xfdf.pdf" /tmp/vibepdf-verify.pdf
cp "Sample PDFs/vibepdf-verify-xfdf.xfdf" /tmp/vibepdf-verify.xfdf
sips -s format png /tmp/vibepdf-verify.pdf --out /tmp/x.png   # CoreGraphics opens it → valid
```

XFDF only (FDF deferred to E1b). Export reads raw dicts; import reuses the
`add_*` writers + patches `/NM`/`/Contents`/`/T` + wires `/IRT` (two-pass). Left
`[~]` pending the human in-app round-trip + cross-reader (open the `.xfdf` in
Acrobat). FDF, freetext font-family fidelity, the O(N·docsize) import re-serialize,
and `<contents-richtext>` are deferred (BACKLOG).

---

### P3.E2 — Flatten annotations (this commit)

No new dependencies. (`lopdf` is used in the new integration test — it's already a
crate dependency and is in scope for integration tests, so no `cargo add`.)

```bash
# Verification gates
npm run check          # tsc + eslint(0 warn) + cargo clippy --all-targets -D warnings → clean
npm run test           # 302/302 (+2: flatten IPC 1, panel flatten 1)
npm run test:rust      # EXIT 0. flatten.rs matrix units: 4 (identity when BBox==Rect,
#   scale+translate onto rect, degenerate bbox skipped, rect-corner ordering).
#   flatten_annotations.rs: 4 (bakes /AP forms into content + drops those annots
#   while keeping the /AP-less note; in-session undo restores all; notes kept;
#   empty-doc safe) + 1 ignored artifact.

# PDF write-path artifact (ignored, on demand) → Sample PDFs/ + /tmp:
cargo test --manifest-path src-tauri/Cargo.toml --test flatten_annotations \
  flatten_writes_verification_artifact -- --ignored
#   → Sample PDFs/vibepdf-verify-flatten.pdf (highlight + rect + ink baked; a note kept).
cp "Sample PDFs/vibepdf-verify-flatten.pdf" /tmp/vibepdf-verify.pdf
sips -s format png /tmp/vibepdf-verify.pdf --out /tmp/x.png   # CoreGraphics opens it → valid
```

COS flatten (not PDFium-native): register each annot's `/AP` form under page
`/Resources /XObject`, append `q <BBox→Rect cm> /name Do Q` to `/Contents`, drop
the annot, prune. `/AP`-less notes/replies kept live. Undoable in-session only
(snapshot inverse). Left `[~]` pending the human in-app + cross-reader pass.
PDFium-native flatten, flattening a subset/by-type, and baking note icons are
deferred (BACKLOG).

---

### P3.C4b — /Measure dict + persisted calibration (this commit)

No new dependencies. First of the "Phase 3.5" deferred sub-features.

```bash
# Verification gates
npm run check          # tsc + eslint(0 warn) + cargo clippy --all-targets -D warnings → clean
npm run test           # 306/306 (+4: measure IPC +1, readMeasureCalibration +1, use-calibration-sync 3,
#   measure-layer assertion updated for the 2 new args)
npm run test:rust      # EXIT 0. measure.rs +2 (writes /Measure dict with /X /C==scale + /U;
#   calibration round-trips via read_measure_calibration; none-without). cos.rs +1
#   (/Measure shape + read-back + none). Six existing add_measure call-sites updated
#   for the +units_per_point/+unit signature (measure.rs, cos.rs, xfdf_roundtrip.rs).

# PDF write-path artifact (ignored, on demand) → Sample PDFs/ + /tmp:
cargo test --manifest-path src-tauri/Cargo.toml --test measure \
  measure_writes_verification_artifact -- --ignored
#   → Sample PDFs/vibepdf-verify-measure.pdf (2 measures, each carrying a /Measure RL dict).
cp "Sample PDFs/vibepdf-verify-measure.pdf" /tmp/vibepdf-verify.pdf
sips -s format png /tmp/vibepdf-verify.pdf --out /tmp/m.png   # CoreGraphics opens it → valid
```

Attach a rectilinear /Measure dict (/X /C = units-per-point, /D 100 = 2-dp) so
readers re-measure live; read it back to re-seed the tool on reopen (no clobber
of an in-session calibration). Imported XFDF measures get a default pt scale.
Left `[~]` pending the human in-app reopen + Acrobat re-measure pass. Angle
formats, anisotropic scale, page /VP viewport, and UTF-16 unit labels deferred.

---

### P3.C3b — image stamps (this commit)

No new dependencies — the `png` crate (added for the render encoder) ships a
decoder in the same crate; PNG decode needed no `cargo add`.

```bash
# Verification gates
npm run check          # tsc + eslint(0 warn) + cargo clippy --all-targets -D warnings → clean
npm run test           # 310/310 (+4: stamps lib +2, stamps IPC +1, stamp-layer image branch +1)
npm run test:rust      # EXIT 0. image_xobject.rs units: 5 (sniff magic, RGB→DeviceRGB
#   XObject no SMask, RGBA→SMask split, reject non-PNG, deinterleave). stamp.rs
#   image cases: +5 (embeds PNG + aspect 2:1 rect, alpha→/SMask, image+text →
#   /Contents, rejects non-PNG, actor round-trip + undo) + the ignored artifact.

# PDF write-path artifact (ignored, on demand) → Sample PDFs/ + /tmp:
cargo test --manifest-path src-tauri/Cargo.toml --test stamp \
  stamp_writes_verification_artifact -- --ignored
#   → Sample PDFs/vibepdf-verify-stamp.pdf (2 text stamps + an image stamp (RGBA,
#     transparent) + an image+text stamp).
cp "Sample PDFs/vibepdf-verify-stamp.pdf" /tmp/vibepdf-verify.pdf
sips -s format png /tmp/vibepdf-verify.pdf --out /tmp/s.png   # CoreGraphics opens it → valid
```

PNG only (JPEG + the bundled default image set deferred). New
`pdf/image_xobject.rs` decodes a PNG → Image XObject, splitting alpha into a
grayscale `/SMask`; `add_image_stamp` places it aspect-correct + clamped, with
an optional overlaid label. `StampSpec` became a `text | image` union. Image
data uncompressed for v1. Left `[~]` pending the human in-app (pick a transparent
PNG → click → renders aspect-correct) + cross-reader pass.

---

### P3.B3b — free-text underline + auto-wrap + double-click re-edit (this commit)

No new dependencies. Last of the "Phase 3.5" deferred sub-features.

```bash
# Verification gates
npm run check          # tsc + eslint(0 warn) + cargo clippy --all-targets -D warnings → clean
npm run test           # 311/311 (+1: free-text-layer double-click re-edit; freetext IPC +underline)
npm run test:rust      # EXIT 0. cos.rs free-text +2 (underline draws a stroke rule + round-trips
#   via /Underline; a long no-\n line wraps to multiple /AP lines + grows the box).
#   ~25 existing add/update_free_text call-sites updated for the +underline signature.

# PDF write-path artifact (ignored, on demand) → Sample PDFs/ + /tmp:
cargo test --manifest-path src-tauri/Cargo.toml --test free_text \
  free_text_writes_verification_artifact -- --ignored
#   → Sample PDFs/vibepdf-verify-freetext-b3b.pdf (bold+breaks, underlined, and a
#     long auto-wrapping box).
cp "Sample PDFs/vibepdf-verify-freetext-b3b.pdf" /tmp/vibepdf-verify.pdf
sips -s format png /tmp/vibepdf-verify.pdf --out /tmp/ft.png   # CoreGraphics opens it → valid
```

Underline (a stroked /AP rule + a private /Underline key for re-edit), auto
word-wrap (shared `wrap_lines` drives both the box height and the drawn lines),
and double-click-to-re-edit (per-box hit-zones → the sidebar's edit flow). Rich
text (/RC + /DS mixed runs) is deferred to B3c, so this lands `[~]`. Left pending
the human in-app + cross-reader pass.

---

### P4.A1 — text-run extraction (this commit) · Phase 4 begins

No new dependencies (uses the existing `pdfium-render` high-level API).

```bash
# Verification gates
npm run check          # tsc + eslint(0 warn) + cargo clippy --all-targets -D warnings → clean
npm run test           # 312/312 (+1: text-runs IPC marshalling)
npm run test:rust      # EXIT 0. text_extract.rs integration: 3 (extracts runs from
#   hello.pdf with sane page-space bbox + populated font/size/colour; out-of-range
#   page errors; stable across links.pdf/forms.pdf) + text_extract.rs units: 3
#   (subset-tag strip ×2, rgb hex).

# No write path → NO /tmp/vibepdf-verify.pdf artifact (A1 is read-only).
```

First Phase-4 step. `pdf/text_extract.rs` reads the **live PdfDocument** under the
PDFium lock (like render_page — not the cos byte path) and emits a TextRun per
text page-object {text, bbox, fontName, embedded, fontSize, color, transform}.
Read-only `ReadTextRuns` actor query + `pdf_extract_text_runs`; `src/ipc/
text-runs.ts` wrapper. The IPC is exposed for B1 (click-to-edit) — no UI consumes
it yet. Left `[~]` (read-only infra, flips to `[x]` when B1 wires it).

---

### P4.A2 — font fallback resolver (this commit)

No new dependencies (pure std::fs scan of OS font dirs; reuses `pdfium-render` +
`lopdf` — the latter only in the test, to build the non-embedded fixture).

```bash
# Verification gates
npm run check          # tsc + eslint(0 warn) + cargo clippy --all-targets -D warnings → clean
npm run test           # 318/318 (+6: fonts IPC marshalling ×1, FontFallbackBanner ×5)
npm run test:rust      # EXIT 0. font_fallback.rs integration: 3 (hand-built non-embedded
#   Calibri → needsFallback + substitute Helvetica; hello.pdf base-14 → no fallback;
#   cross-doc substitute-iff-fallback invariant) + font_resolver.rs units: 8 (embedded/
#   standard/system-available/fallback buckets, substitute family+style, normalize
#   collapses variants, report dedup + roll-up).

# No write path → NO /tmp/vibepdf-verify.pdf artifact (A2 is read-only).
```

`pdf/font_resolver.rs` is a **pure** resolver (`resolve_font` + `build_font_report`
over an injected `SystemFontIndex`); the only side effect is `load_system_fonts`, a
one-time `OnceLock`-cached dir scan — no network, no new dep. `text_extract.rs` gains
`collect_document_fonts` (distinct (name, embedded), live-PDFium read). Read-only
`ReadFontReport` actor query + `pdf_read_font_report`; `src/ipc/fonts.ts` +
`use-font-report.ts` (once-per-doc, keyed on document id) + `FontFallbackBanner.tsx`
(disabled re-flow affordance until B1). Left `[~]` (re-flow action + banner eyeball
land with B1).

---

### P4.A3 — in-place text editing (this commit) · shipped EDIT-only

No new dependencies (uses the existing `pdfium-render` mutation API; `lopdf` in the
test to build the non-embedded fixture).

```bash
# Verification gates
npm run check          # tsc + eslint(0 warn) + cargo clippy --all-targets -D warnings → clean
npm run test           # 318/318 (frontend untouched by A3)
npm run test:rust      # EXIT 0. reflow.rs: 5 (replace preserves position + changes text
#   via A1 re-extraction; round-trips through PDFium; edits a non-embedded font; bad
#   page/run index errors; ReplaceTextRunEdit inverse restores) + 1 ignored artifact.

# Write path → verification artifact:
cargo test --test reflow writes_verification_artifact -- --exact --ignored
#   → /tmp/vibepdf-verify.pdf (hello.pdf edited "Hello, VibePDF." → "Hello, World!")

# Diagnosis that drove the EDIT-only scope cut (not committed; for the record):
#   Bracketed FPDFPage_RemoveObject with stderr markers → "before remove" prints,
#   process SIGSEGVs, "after remove" never reached. Reproduced with 1 and 2 page
#   loads → it's the bundled PDFium, not our code. set_text path is unaffected.
```

`pdf/reflow.rs::replace_text_run` rewrites a run's text **in place** via PDFium
`FPDFText_SetText` on a *throwaway* doc (never the live doc — content mutation
SIGSEGVs at teardown), staged under `Manual` regeneration + one `regenerate_content()`
(without it `set_text` is silently dropped on save), then serialized.
`ReplaceTextRunEdit` swaps the live doc with a `RestoreDocEdit` snapshot inverse. No
actor/IPC/UI (B1 wires it). **Redact half (delete / true redaction / fallback recreate)
deferred** — `FPDFPage_RemoveObject` SIGSEGVs in our PDFium (2026-06-26 decision: ship
edit-only, removal → future lopdf content-stream surgery). Left `[~]`.

---

### P4.B1 — click-to-edit text (this commit)

No new dependencies (pure wiring over A1/A2/A3).

```bash
# Verification gates
npm run check          # tsc + eslint(0 warn) + cargo clippy --all-targets -D warnings → clean
npm run test           # 324/324 (+6: text-edit IPC ×1, cssFamilyForFont ×1, TextEditLayer ×4)
npm run test:rust      # EXIT 0. text_edit.rs: 3 (actor replace changes text + records undo;
#   undo restores original; out-of-range run errors) + 1 ignored artifact.

# Write path → verification artifact (full B1 path: actor edit → save):
cargo test --test text_edit writes_verification_artifact -- --exact --ignored
#   → /tmp/vibepdf-verify.pdf (hello.pdf edited "Hello, VibePDF." → "Hello, World!")
```

`ReplaceTextRun` actor message applies A3's `ReplaceTextRunEdit` (record inverse, dirty,
return `HistoryState`); `pdf_replace_text_run` command + `src/ipc/text-edit.ts`. Frontend
`TextEditLayer` (overlay in `PageVirtualizer`) fetches runs via A1, lays hit-zones, opens
an inline editor on click, commits via `replaceTextRun` → `bumpEpoch`. **Edit Text** tool
toggle in `MarkupToolbar`; `edit-text` added to `ToolId`. Non-embedded run shows an inline
A2 cue. Left `[~]` pending the in-app eyeball (which also flips A1/A2/A3 → `[x]`).

---

### P4.B3 — delete text via lopdf content-stream surgery (this commit)

No new dependencies (uses existing `lopdf` content API; `pdfium-render` for the verify).

```bash
# Verification gates
npm run check          # tsc + eslint(0 warn) + cargo clippy --all-targets -D warnings → clean
npm run test           # 326/326 (+2: deleteTextRun marshalling, Delete-button interaction)
npm run test:rust      # EXIT 0. text_delete.rs: 5 (removes from hello.pdf, verified by
#   re-extraction; 2-run fixture proves ordinal correctness ×2; XObject text fails safely;
#   out-of-range errors) + 1 ignored artifact. text_edit.rs: +1 (delete_then_undo_restores).

# Write path → verification artifact:
cargo test --test text_delete writes_verification_artifact -- --exact --ignored
#   → /tmp/vibepdf-verify.pdf (hello.pdf with its only run deleted)
```

Sidesteps the A3 `FPDFPage_RemoveObject` SIGSEGV: `pdf/reflow.rs::delete_text_run` removes a
run at the **lopdf COS level** — `get_and_decode_page_content` → drop the run's `Tj`/`TJ`
operator → `change_page_content`. **Verified by re-extraction** (post == pre minus the target,
else error — also P6-SEC-010(c)); `'`/`"` + XObject text rejected. `DeleteTextRun` actor msg +
`pdf_delete_text_run` + `deleteTextRun` IPC + **Delete** button in the Edit Text editor (B3 UI).
Undoable. **Unblocks** the standing `FPDFPage_RemoveObject` blocker for delete; P6 redaction
will reuse this primitive. Left `[~]` pending the in-app eyeball.

---

### P4.B2 — add text box as page content (this commit)

No new dependencies (reuses the lopdf content API + free-text's drawing).

```bash
# Verification gates
npm run check          # tsc + eslint(0 warn) + cargo clippy --all-targets -D warnings → clean
npm run test           # 330/330 (+4: addTextBox marshalling, TextBoxLayer ×3)
npm run test:rust      # EXIT 0. text_add.rs: 5 (added text extracts as a CONTENT run not an
#   annotation [read_annotations unchanged]; F1-collision fixture keeps its text; empty
#   rect/text errors; actor add+undo) + 1 ignored artifact. free_text.rs still 8 (the
#   free_text_appearance_content font-name parameterization didn't regress).

# Write path → verification artifact:
cargo test --test text_add writes_verification_artifact -- --exact --ignored
#   → /tmp/vibepdf-verify.pdf (hello.pdf + an added content text box)
```

`cos::add_text_box` registers a base-14 font under a collision-free `Fvibe…` name (cloning a
shared/inherited `/Resources` so it never mutates another page's), then appends the same
`q BT … Tj … ET … Q` fragment free-text draws — but into the **page content stream**, not an
annotation (the spec's key clause). `TextBoxEdit` (cos_edit + RestoreDocEdit) + `AddTextBox`
actor msg + `pdf_add_text_box` + `addTextBox` IPC + a drag-to-create `TextBoxLayer` + the
**Add Text** tool (reusing the free-text style controls). The result is real content, so it's
editable/deletable via B1/B3 with no extra path. Left `[~]` pending the in-app eyeball.

---

### P4.C1 — add image as page content (this commit) · Track C begins

No new dependencies (reuses `png` + lopdf; JPEG embedded verbatim, no decoder).

```bash
# One-time fixture generation (committed):
#   python3 → tiny PNG → sips -s format jpeg → tests/fixtures/basic/sample.jpg

# Verification gates
npm run check          # tsc + eslint(0 warn) + cargo clippy --all-targets -D warnings → clean
npm run test           # 333/333 (+3: addImage marshalling, ImageAddLayer ×2)
npm run test:rust      # EXIT 0. image_add.rs: 5 (PNG → content XObject painted by Do, NOT an
#   annotation, round-trips; real sample.jpg → /DCTDecode + PDFium round-trip; two adds → two
#   distinct XObject names; GIF/garbage/empty-rect error; actor add+undo) + 1 ignored artifact.
#   image_xobject.rs units: +3 (JPEG magic, SOF→DCTDecode dims, embed_image dispatch).
#   text_add/free_text/stamp unaffected by the register_page_resource/append_page_content refactor.

# Write path → verification artifact:
cargo test --test image_add writes_verification_artifact -- --exact --ignored
#   → /tmp/vibepdf-verify.pdf (hello.pdf + an embedded PNG)
```

`image_xobject::embed_jpeg` embeds a JPEG verbatim as a `/DCTDecode` stream (dims from the SOF
header — no decode); `embed_image` dispatches PNG/JPEG by magic. `cos::add_image` embeds →
registers the XObject under a collision-free `Imgvibe…` name → appends `q <cm> /Img Do Q` to the
page content (aspect-fit). The Resource-registration + content-append helpers were generalized
out of B2 (`register_page_resource`, `append_page_content`). `AddImageEdit` + `AddImage` actor
msg + `pdf_add_image` (reads the file) + `addImage` IPC + pick-then-arm (`image-add-store`) +
drag-to-place `ImageAddLayer` + **Add Image** toolbar button (file dialog). Real content → C2
will edit it. PNG+JPEG only; rotation → C2. Left `[~]` pending the in-app eyeball.

---

### P4.C2 — edit existing image: move/resize/rotate/delete (this commit)

No new dependencies.

```bash
# Verification gates
npm run check          # tsc + eslint(0 warn) + cargo clippy --all-targets -D warnings → clean
npm run test           # 344/344 (+11: matrix ×4, image-edit IPC ×3, ImageEditLayer ×4)
npm run test:rust      # EXIT 0. image_edit.rs: 7 (RISK-#1 transform no-SIGSEGV; locate; delete +
#   2-image ORDINAL correctness; rotate-90 aspect swap; out-of-range; actor transform+delete+undo)
#   + 1 ignored artifact. text_add/image_add/text_delete/free_text unaffected by the
#   append_page_content `\n` fix.

# Write path → verification artifact:
cargo test --test image_edit writes_verification_artifact -- --exact --ignored
#   → /tmp/vibepdf-verify.pdf (an image moved + resized)
```

RISK #1 RESOLVED: PDFium `reset_matrix` is a mutate-in-place FFI (like set_text) — works, no
crash. `image_extract::extract_images` (A1-style read) + `image_edit::transform_image`
(reset_matrix, throwaway doc + Manual regen) covers move/resize/rotate via one matrix the
frontend computes; `delete_image` splices the image's `Do` at the lopdf level (B3-style, verified
by re-extraction). `ReadImages`/`TransformImage`/`DeleteImage` actor + 3 commands + `image-edit.ts`
+ `matrix.ts` (pure math) + selection-box `ImageEditLayer` + **Edit Image** tool. **Bug found +
fixed:** `append_page_content` lacked a leading separator → `…ET`+`q` = `ETq` on lopdf re-decode
(PDFium masked it), corrupting multi-image delete; prepend `\n`. Replace → C2b. Left `[~]`.

---

### P4.C2b — replace an image's pixels (this commit)

No new dependencies.

```bash
# Verification gates
npm run check          # tsc + eslint(0 warn) + cargo clippy --all-targets -D warnings → clean
npm run test           # 346/346 (+2: replaceImage marshalling, Replace-button → file pick)
npm run test:rust      # EXIT 0. image_replace.rs: 5 (swap preserves placement + changes XObject
#   dims; keeps alpha [RGBA → /SMask]; preserves other images [ordinal]; out-of-range; actor
#   replace+undo) + 1 ignored artifact.

# Write path → verification artifact:
cargo test --test image_replace writes_verification_artifact -- --exact --ignored
#   → /tmp/vibepdf-verify.pdf (an image replaced with a different one)
```

Completes P4-EDIT-006. `image_edit::replace_image` embeds the new image (`embed_image`) and
**overwrites the XObject in place** at the id the selected image's `Do` references — name/`cm`/`Do`
untouched, so no `/Resources` edit / copy-on-write. Verified by re-extraction (count + every bbox
unchanged). `ReplaceImageEdit` + `ReplaceImage` actor msg + `pdf_replace_image` (reads the file) +
`replaceImage` IPC + a **Replace** button (file dialog) in the selection toolbar. Placement
preserved (new image fills the old box). Left `[~]` (C2's eyeball now also covers replace).

---

### P4.C3 — Hyperlinks (this commit)

No new dependencies.

```bash
# Verification gates
npm run check          # tsc + eslint(0 warn) + cargo clippy --all-targets -D warnings → clean
npm run test           # 353/353 (+7: target.ts validation + 1-based→0-based wire conversion)
npm run test:rust      # EXIT 0. link.rs: 8 (url roundtrip; mailto prefix; internal-page /Dest;
#   named dest kept; URL-with-parens escaped; out-of-range page errs; unknown kind errs;
#   actor add+undo) + 1 ignored artifact.

# Write path → verification artifact:
cargo test --test link link_writes_verification_artifact -- --ignored
#   → ../Sample PDFs/vibepdf-verify-link.pdf (copied to /tmp/vibepdf-verify.pdf);
#   3 links on links.pdf page 1: a URL, a page-jump (→ p2), and a mailto.
```

Completes P4-EDIT-007 (Track C). `cos::add_link` builds a `/Link` annotation dict — url/email →
`/A /URI`, internal page → `/Dest [pageRef /Fit]` (the form the P2 reorder/delete fixups already
resolve), named → `/Dest (name)`; invisible hot-zone (`/Border [0 0 0]`, no `/AP`); `(value)`
escaped by `string_literal`. `AddLinkEdit` (via `cos_edit`) + `AddLink` actor msg + `pdf_add_link`
+ `addLink` IPC + `tools/link/target.ts` (pure validate + wire conversion) + `LinkLayer` (drag rect
→ target popover) + an **Add Link** toolbar button. Primitive lives in `cos.rs` (convention), not a
new `link.rs`. Left `[~]` (awaiting the cross-reader eyeball).

---

### P4.C3b — Link appearance (this commit)

No new dependencies. **New spec line P4-EDIT-007b** added to `docs/02_PRODUCT_SPEC.md`
(human-approved wording).

```bash
# Verification gates
npm run check          # tsc + eslint(0 warn) + cargo clippy --all-targets -D warnings → clean
npm run test           # 355/355 (+2: default style=box/#0000ff, style list = box/underline/invisible)
npm run test:rust      # EXIT 0. link.rs: 14 (+6: invisible no-/AP; box /AP+/C+/BS S; underline /BS U;
#   styled still navigates; unknown style errs; bad colour errs) + 1 ignored artifact.

# Write path → verification artifact:
cargo test --test link link_writes_verification_artifact -- --ignored
#   → ../Sample PDFs/vibepdf-verify-link.pdf (→ /tmp/vibepdf-verify.pdf): a blue box URL link, a
#   red underline page link (0-based "1" → page 2), and an invisible mailto. Confirmed via Apple
#   PDFKit: box→URL, underline→page 2, invisible→mailto.
```

Extends P4-EDIT-007 with appearance. `add_link` gains `style` (`box` default / `underline` /
`invisible`) + `color`; `apply_link_appearance` attaches a generated `/AP` Form XObject (1pt stroke,
BBox==Rect identity matrix, inset by half line width) + `/C` + `/BS` for visible styles, or leaves
`/Border [0 0 0]` (byte-identical to C3) for invisible. Threaded through `AddLinkEdit` / `AddLink` /
`pdf_add_link` / `addLink` IPC; popover gains a Style select + `<input type=color>`. Default moved to
a **visible box** per the human. Left `[~]`.

---

### P4.D2 — Watermark (this commit)

No new dependencies. New 50-page fixture generated once:

```bash
python3 tests/fixtures/basic/generate-many.py   # → many-pages.pdf (50 pages, 13522 B)

# Verification gates
npm run check          # tsc + eslint(0 warn) + cargo clippy --all-targets -D warnings → clean
npm run test           # 359/359 (+4: parsePageRange all/range/dedupe/errors + defaults)
npm run test:rust      # EXIT 0. watermark.rs: 10 (selected-pages-only; behind prepends / on-top
#   appends; opacity /ExtGState; rotation cm 0.70711; image embeds once; empty-pages / empty-text /
#   out-of-range errors; 50-page <2s [measured 0.12s]; actor add+undo) + 1 ignored artifact.

# Write path → verification artifact:
cargo test --test watermark watermark_writes_verification_artifact -- --ignored
#   → ../Sample PDFs/vibepdf-verify-watermark.pdf (→ /tmp/vibepdf-verify.pdf): "DRAFT" behind every
#   page of the 50-page fixture. Apple PDFKit confirms pages 1/26/50 render DRAFT + original text.
```

Opens Track D. `watermark.rs` (new module) `add_watermark` stamps text/image page content per
selected page: an opacity `/ExtGState` + base-14 font (or a once-embedded image `XObject`) and a
`q…Q` fragment rotated about the page centre (`cm`). On-top = `append_page_content`, behind =
`prepend_page_content` (new, mirror of append). Self-contained `WatermarkEdit` (snapshot→reload,
`RestoreDocEdit`). Five cos helpers promoted to `pub(crate)`. `AddWatermark` message +
`pdf_add_text_watermark` / `pdf_add_image_watermark` (reads file) + `WatermarkDialog` (document-wide,
mounted in PdfViewer, opened from ZoomToolbar) + `parsePageRange`. Left `[~]`.

---

### P4.D1a — Background (colour / image) (this commit)

No new dependencies.

```bash
# Verification gates
npm run check          # tsc + eslint(0 warn) + cargo clippy --all-targets -D warnings → clean
npm run test           # 359/359 (parsePageRange cases relocated to tools/__tests__/page-range.test.ts)
npm run test:rust      # EXIT 0. background.rs: 8 (color fills behind content; selected-pages-only;
#   image embeds once + clips [W n]; opacity /ExtGState GSbg; empty-pages / bad-colour /
#   out-of-range errors; actor add+undo) + 1 ignored artifact.

# Write path → verification artifact:
cargo test --test background bg_writes_verification_artifact -- --ignored
#   → ../Sample PDFs/vibepdf-verify-background.pdf (→ /tmp/vibepdf-verify.pdf): #e6f0ff behind
#   hello.pdf. Apple PDFKit render: corner pixel = (230,240,255) = #e6f0ff (paints behind the text).
```

First reuse of Track D's shared machinery. New `background.rs` `add_background` prepends a full-page
`q…Q`: colour fills the MediaBox rect (`re f`), image is embedded once + drawn **cover-fit with a
clip** (`re W n` + cover `cm` `Do`). Always behind (`prepend_page_content`). Self-contained
`BackgroundEdit`. **Consolidations:** `page_media_box` → `cos.rs` `pub(crate)` (was in watermark.rs);
`parsePageRange` → `src/tools/page-range.ts` (watermark re-exports). `AddBackground` message +
`pdf_add_color_background` / `pdf_add_image_background` (reads file) + `BackgroundDialog`. **PDF-page
source deferred to D1b** (cross-doc page → Form XObject). Left `[~]`.

---

### P4.D1b — Background from a PDF page (this commit)

No new dependencies.

```bash
# Verification gates
npm run check          # tsc + eslint(0 warn) + cargo clippy --all-targets -D warnings → clean
npm run test           # 359/359 (no frontend test delta — D1b reuses parsePageRange)
npm run test:rust      # EXIT 0. background.rs: 14 (8 D1a + 6 D1b: imports Form behind content;
#   copies page /Resources + content; embeds once across pages; subtree-copy-not-whole-source;
#   source-page out-of-range; actor add+undo) + 1 ignored artifact.

# Write path → verification artifact:
cargo test --test background bg_writes_verification_artifact -- --ignored
#   → ../Sample PDFs/vibepdf-verify-background.pdf (→ /tmp/vibepdf-verify.pdf): a colour wash + 
#   links.pdf page 1 (35% opacity) behind hello.pdf. Apple PDFKit text extraction:
#   "Page 1 (link to page 3) Hello, VibePDF." — imported page renders, behind, with its font.
```

Completes P4-EDIT-008 (Track-D background). `import_page_as_form` renumbers the source's ids above
the dest's (`renumber_objects_with`), copies **only** the chosen page's resource object closure (BFS
over references — not the whole source), and wraps the page content in a `/Form` XObject (`BBox` =
source `MediaBox`); each target page references that one Form, drawn **contain-fit** + centred.
`BackgroundKind::PdfPage` + `pdf_add_pdf_background` (reads source file; reuses the `AddBackground`
message) + the dialog's "PDF page" source. Source `/Rotate` ignored (documented). Left `[~]`.

---

### P4.D3 — Header / footer (this commit)

No new dependencies (`{date}` value is supplied by the frontend — no Rust date lib).

```bash
# Verification gates
npm run check          # tsc + eslint(0 warn) + cargo clippy --all-targets -D warnings → clean
npm run test           # 359/359 (no frontend test delta — reuses parsePageRange)
npm run test:rust      # EXIT 0. header_footer.rs: 9 (substitute unit; footer "Page {n} of {total}";
#   header-high/footer-low y; L/C/R distinct x; only-non-empty positions; all-empty / unknown-
#   position / out-of-range errors; actor add+undo) + 1 ignored artifact.

# Write path → verification artifact:
cargo test --test header_footer hf_writes_verification_artifact -- --ignored
#   → ../Sample PDFs/vibepdf-verify-header-footer.pdf (→ /tmp/vibepdf-verify.pdf): footer
#   "Page {n} of {total}" + "{date}" on all 50 pages. Apple PDFKit per-page text:
#   p1 "Page 1 of 50 2026-07-01", p25 "Page 25 of 50 …", p50 "Page 50 of 50 …".
```

New `header_footer.rs` `add_header_footer` draws L/C/R text in the top/bottom margin as **appended**
page content (overlays). Per page, each non-empty position's `{n}`/`{total}`/`{date}` are substituted
(pure `substitute`, unit-tested) and drawn at the aligned x + header/footer baseline y. `{date}` value
comes from the frontend (its formatted today) — no Rust date dep. **Consolidation:** `escape_pdf_string`
→ `cos.rs` `pub(crate)`. `AddHeaderFooter` message + `pdf_add_header_footer` + `HeaderFooterDialog`
(three fields + placeholder hint). Left `[~]`.

---

### P4.HF — FABLE_REVIEW bug batch (this commit)

No new dependencies. Two fixtures generated once:

```bash
python3 tests/fixtures/basic/generate-rotated.py   # → rotated.pdf (4pp, /Rotate 0/90/180/270)
python3 tests/fixtures/basic/generate-cropped.py   # → cropped.pdf (CropBox ⊂ MediaBox)

# Verification gates
npm run check          # tsc + eslint(0 warn) + cargo clippy --all-targets -D warnings → clean
npm run test           # 359/359 (no frontend delta — engine-only batch)
npm run test:rust      # EXIT 0. +9: per-angle compensating-cm tests (watermark 2, background 2,
#   header_footer 2), hardening.rs: /Contents ref→array preserved; encrypted open→save pin.

# Write path → verification artifact:
cargo test --test hardening hf_writes_verification_artifact -- --ignored
#   → ../Sample PDFs/vibepdf-verify-hardening.pdf (→ /tmp/vibepdf-verify.pdf): DRAFT watermark
#   + "Page {n} of {total}" footer on all 4 rotated pages. Apple PDFKit: every page carries both.
```

Fixes FABLE_REVIEW 3.1/3.4/3.7/3.3. Writers lay out in **visual space** (`page_rotation` +
`page_effective_box` + `visual_transform`) → upright, crop-aware decorations; colour fill stays
MediaBox. `existing_contents` derefs a `/Contents` reference-to-array. **3.3's pin found the real
bug:** encrypted docs couldn't be saved at all (verify re-opened the still-encrypted temp with no
password) — `save_document` now takes the open password; save works and encryption is preserved.
FABLE_REVIEW annotated with ✅ FIXED/RESOLVED notes. Left `[~]`.

---

### P4.HF2 — Marked-content tags on decorations (this commit)

No new dependencies.

```bash
# Verification gates
npm run check          # tsc + eslint(0 warn) + cargo clippy --all-targets -D warnings → clean
npm run test           # 359/359 (engine-only)
npm run test:rust      # EXIT 0. +4: watermark/background/header_footer *_is_tagged +
#   hardening.rs decoration_tag_is_operator_spliceable (removes the tagged watermark by
#   operator splice; original content intact; PDFium reopens).

# Write path → verification artifact (regenerated with tags):
cargo test --test hardening hf_writes_verification_artifact -- --ignored
#   → /tmp/vibepdf-verify.pdf. NOTE: PDFium compresses content streams on save — the
#   /VibePDF tag is found at the operator layer (get_and_decode_page_content), not by grep.
```

FABLE_REVIEW 3.13. `cos::wrap_decoration` wraps every Track-D fragment in
`/VibePDF << /Kind (…) /Id (uuid) >> BDC … EMC` — the content-stream `/NM`. Future
removal/re-stamp = mechanical operator splice (proven by test). D4/D5 inherit via the shared
writers. Left `[~]`.

---

### P4.HF3 — WinAnsi text + error toasts (this commit)

No new dependencies.

```bash
# Verification gates
npm run check          # tsc + eslint(0 warn) + cargo clippy --all-targets -D warnings → clean
npm run test           # 368/368 (+9: toast-store ×4, report-error ×5)
npm run test:rust      # EXIT 0. winansi.rs (9): Latin-1/CP1252 → octal + /WinAnsiEncoding;
#   ASCII byte-stable; parens/backslash escaped; reject CJK per entry (watermark/header-footer/
#   text-box/free-text add+update); error names ≤3 offenders. + 1 ignored artifact.

# Write path → verification artifact:
cargo test --test winansi winansi_writes_verification_artifact -- --ignored
#   → ../Sample PDFs/vibepdf-verify-winansi.pdf (→ /tmp/vibepdf-verify.pdf): "Café résumé"
#   watermark + "Página 1 – 50 %" footer + "naïve € 5" free text. Apple PDFKit render:
#   Café/résumé/Página/– all correct (annotation-AP text not in page.string, expected).
```

FABLE_REVIEW **3.2 stage 1** + **3.5**. Text writers: `base14_font_dict` sets
`/Encoding /WinAnsiEncoding`; `escape_pdf_string` transcodes Latin-1/CP1252 → octal;
`ensure_winansi` rejects non-WinAnsi at all 7 text entries with a character-naming error (old
`pdf_escape` collapsed in). Frontend: `toast-store` + `Toasts` (App-mounted) + `report-error`
(`CommandError.code`→copy); 21 user-action `console.warn` catches → `reportError`. Font embedding
(3.2 stage 2, real Unicode) still deferred. Left `[~]`.

---

### P4.HF4 — collect_refs recursion → worklist (this commit)

No new dependencies.

```bash
# Verification gates
npm run check          # tsc + eslint(0 warn) + cargo clippy --all-targets -D warnings → clean
                       #   (fixed: collapsible_match → match guard; doc_markdown backtick)
npm run test:rust      # EXIT 0, 364 passed (+2). New: background::tests::
#   collect_refs_survives_deep_reference_chain (100k container links, 0.3 s) +
#   …_terminates_on_a_reference_cycle. Frontend untouched (no npm run test needed).

# Write path (D1b import) → verification artifact:
cargo test --test background bg_writes_verification_artifact -- --ignored
#   → ../Sample PDFs/vibepdf-verify-background.pdf (→ /tmp/vibepdf-verify.pdf): links.pdf p1
#   imported behind hello.pdf; PDFium-verified via save's verify_pdf_reopens.
```

FABLE_REVIEW **3.14**. `background.rs::collect_refs` (walks the untrusted source PDF's resource
graph in D1b import) is now iterative — `pending` id worklist + per-object `inline` stack — so a
crafted deep container chain can't overflow the actor thread's stack; `acc` still bounds each id
to one visit and behaviour on valid input is unchanged. Learned: lopdf `get_object` collapses bare
`M 0 R` chains, so the overflow shape is *container* links, not bare refs (the test uses that).
Left `[~]`.

---

### P4.HF5 — Font embedding stage-2, tracer on header/footer (this commit)

No new dependencies (embedding rides the already-linked PDFium engine). One committed asset:
a 28 KB SIL-OFL fixture font.

```bash
# Verification gates
npm run check          # tsc + eslint(0 warn) + cargo clippy --all-targets -D warnings → clean
                       #   (fixed: doc_markdown backticks via `clippy --fix`; one many_single_char_names allow)
npm run test:rust      # EXIT 0, 367 passed (+3). New inline: font_embed::tests (embed round-trips +
#   re-extracts Coptic; preserves existing base-14 text), header_footer::tests (embedded footer
#   renders + extracts, page text intact). winansi.rs non_winansi_header_footer_rejected → _now_embeds.

# Write path (embedded header/footer) → verification artifact:
cargo test --test header_footer hf_embedded_unicode_verification_artifact -- --ignored
#   → ../Sample PDFs/vibepdf-verify-hf-unicode.pdf (→ /tmp/vibepdf-verify.pdf): Greek + Cyrillic
#   footer on 3 pages. Structure check: /Type0 + /CIDFontType2 + /ToUnicode + /FontFile2 present.
#   NOTE: 15 MB — PDFium embeds the FULL covering font (no subsetting). Top follow-up.
```

FABLE_REVIEW **3.2 stage-2 (start)**. `cos::ensure_winansi` → branch via `cos::winansi_fits`:
WinAnsi text keeps the base-14 lopdf path; non-WinAnsi routes to new `font_embed::embed_runs`
(PDFium `load_true_type_from_bytes` + text objects → `/Type0`/`/CIDFontType2`/`/ToUnicode`).
`font_resolver::covering_font_bytes` supplies a best-effort broad system face; falls back to the
HF3 rejection when none. Tracer wired into **header/footer** only. Follow-ups: font subsetting
(the size fix), the other 6 writers, per-glyph coverage, HF2 tag on embedded runs, exact metrics
(3.10). Left `[~]`.

---

### P4.HF6 — Font subsetting, the HF5 size fix (this commit)

**New dependencies** (the "no font parser" call, finally paid — justified in docs/03):
`subsetter = "0.1"` + `ttf-parser = "0.25"`, both **MIT/Apache-2.0, zero transitive deps**.

```bash
# Dependency add + audit (pinned 0.1 — the 0.2.x subsetter drags in the fontations
# stack (skrifa/read-fonts/write-fonts/kurbo/euclid, 11 crates) and needs rustc 1.85 > our 1.80).
cargo add subsetter@0.1 ttf-parser
cargo tree -p subsetter    # → subsetter v0.1.1 (no children)

# Verification gates
npm run check          # tsc + eslint(0 warn) + cargo clippy --all-targets -D warnings → clean
                       #   (doc_markdown backticks via clippy --fix)
npm run test:rust      # EXIT 0, 368 passed (+1). New: font_embed::tests::
#   subset_shrinks_font_and_still_embeds_unicode — subset < full AND PDFium re-extracts through it.

# Write path → verification artifact (regenerated HF5 footer):
cargo test --test header_footer hf_embedded_unicode_verification_artifact -- --ignored
#   → Sample PDFs/vibepdf-verify-hf-unicode.pdf (→ /tmp/vibepdf-verify.pdf).
#   SIZE: 15 MB → 60 KB (~256×). Structure intact: /Type0 /CIDFontType2 /ToUnicode /FontFile2.
```

FABLE_REVIEW **3.2 stage-2 size fix**. PDFium embeds the whole face; its native
`FPDF_SUBSET_NEW_FONTS` save flag is unreachable through pdfium-render 0.9.1 (handle + file-writer
`pub(crate)`, `flags` hardcoded 0). So `font_embed::subset_font` subsets the face to just the runs'
glyphs before `load_true_type_from_bytes` (ttf-parser: codepoints→gids; subsetter: `Profile::pdf`,
which keeps original gids + cmap so PDFium still resolves Unicode). Unparseable faces embed whole
(correct, not small). Left `[~]`.

---

### P4.HF7 — Watermark text embedding (this commit)

No new dependencies.

```bash
# Verification gates
npm run check          # tsc + eslint(0 warn) + cargo clippy --all-targets -D warnings → clean
                       #   (doc_markdown backticks via clippy --fix; float_cmp allow on the compose test)
npm run test:rust      # EXIT 0, 370 passed. New inline: font_embed opacity/behind spikes,
#   watermark embed + behind + winansi-base14-unchanged + compose. winansi non_winansi_watermark
#   → _now_embeds; error_names test repointed to add_text_box (still rejects).

# Write path → verification artifact:
cargo test --test watermark watermark_embedded_unicode_verification_artifact -- --ignored
#   → Sample PDFs/vibepdf-verify-watermark-unicode.pdf (→ /tmp/vibepdf-verify.pdf): "Черновик
#   Πρόχειρο" behind, opacity 0.3, rotated 45°, 3 pages. 56 KB (subsetted). /Type0 + /ToUnicode.
```

FABLE_REVIEW **3.2 stage-2**, second writer. `EmbedRun` + `opacity` (→ PDFium fill alpha) + `behind`
(→ `insert_object_at_index(0)`); `cos::compose` bakes `vt·R@centre·Td` into the run matrix.
`watermark::add_watermark` branches on `winansi_fits` → `add_watermark_embedded` for non-WinAnsi;
WinAnsi keeps the base-14 `/ExtGState` path unchanged. 4 text writers remain (text box, free-text
×2, stamp/image-stamp). Left `[~]`.

---

### P4.HF8 — Text-box text embedding (this commit)

No new dependencies.

```bash
# Verification gates
npm run check          # tsc + eslint(0 warn) + cargo clippy --all-targets -D warnings → clean
                       #   (doc_markdown backticks via clippy --fix)
npm run test:rust      # EXIT 0, 378 passed. New: font_embed underline path spike;
#   cos::text_box_embed_tests (wrap into >1 run, one underline rule/line, base-14 unchanged).
#   winansi non_winansi_text_box → _now_embeds; error_names repointed text-box → free-text.

# Write path → verification artifact:
cargo test --test text_box text_box_embedded_unicode_artifact -- --ignored
#   → Sample PDFs/vibepdf-verify-text-box-unicode.pdf (→ /tmp/vibepdf-verify.pdf): multi-line
#   underlined Russian text box. 48 KB (subsetted). /Type0 + /ToUnicode.
```

FABLE_REVIEW **3.2 stage-2**, third (last page-content) writer. `EmbedRun.underline: Option<f32>`
drawn as a PDFium `create_path_object_line` rule under the run matrix. `cos::add_text_box_embedded`
reuses the base-14 `wrap_lines`/`free_text_inner_width` layout → one run per wrapped line;
`add_text_box` branches on `winansi_fits`. WinAnsi keeps `free_text_appearance_content`. 3
annotation-`/AP` writers remain (free-text ×2, stamp). Left `[~]`.

---

### P4.HF9 — Free-text /AP font embedding via hand-built CID font (this commit)

No new dependencies (reuses subsetter + ttf-parser from HF6).

```bash
# Verification gates
npm run check          # tsc + eslint(0 warn) + cargo clippy --all-targets -D warnings → clean
                       #   (doc_markdown backticks via clippy --fix; module allow for font-metric casts)
npm run test:rust      # EXIT 0, 384 passed. New: font_embed_cid (spike: PDFium renders + extracts a
#   hand-built CID font; dict shape; Identity-H encoding). cos free_text_embed_tests (/AP has CID
#   font; re-edit updates /Contents; WinAnsi keeps base-14 /AP). winansi free_text → _now_embeds;
#   error_names repointed free-text → stamp.

# Write path → verification artifact:
cargo test --test free_text free_text_embedded_unicode_artifact -- --ignored
#   → Sample PDFs/vibepdf-verify-freetext-unicode.pdf (→ /tmp/vibepdf-verify.pdf): Cyrillic + Greek
#   underlined free-text. 24 KB. /Type0 + /CIDFontType2 + /ToUnicode + /FontFile2.
```

FABLE_REVIEW **3.2 stage-2**, the annotation-`/AP` class (PDFium page-objects can't reach an /AP).
New `font_embed_cid::build_cid_font` hand-builds Type0/CIDFontType2 in lopdf (subset via subsetter,
metrics/advances via ttf-parser, /FontFile2 + /FontDescriptor + /W + /ToUnicode, Identity-H).
`cos::free_text_appearance` (shared by add + update) branches on `winansi_fits` → CID `/AP` with
`<gid> Tj`; /Contents keeps the plain text for re-edit. Only stamp labels remain. Left `[~]`.

---

### P4.HF10 — Stamp label embedding, stage-2 writer surface complete (this commit)

No new dependencies (reuses HF9's build_cid_font).

```bash
# Verification gates
npm run check          # tsc + eslint(0 warn) + cargo clippy --all-targets -D warnings → clean
                       #   (doc_markdown backticks via clippy --fix; renamed l→lbl for many_single_char_names)
npm run test:rust      # EXIT 0, 388 passed. New: cos stamp_embed_tests (stamp /AP CID, base-14 kept,
#   ensure_winansi names ≤3 — graduated); stamp.rs image_stamp_unicode_label. winansi
#   error_names → removed; non_winansi_stamp_now_embeds added.

# Write path → verification artifact:
cargo test --test stamp stamp_embedded_unicode_artifact -- --ignored
#   → Sample PDFs/vibepdf-verify-stamp-unicode.pdf (→ /tmp/vibepdf-verify.pdf): Cyrillic text stamp +
#   Greek image-stamp label. 40 KB. /Type0 + /CIDFontType2 + /ToUnicode.
```

FABLE_REVIEW **3.2 stage-2**, the last writers. `stamp_appearance_content` / `image_stamp_content`
gained a `show: Fn(&str)->String` closure (base-14 `(literal)` vs CID `<hex>`); `add_stamp` /
`add_image_stamp` branch on `winansi_fits(label.to_uppercase())` → `build_cid_font`. **All 7
rendered-text entries now embed Unicode** — no writer rejects unconditionally, so the char-naming
test graduated to a direct `ensure_winansi` unit test. Stage-2 writer surface complete. Left `[~]`.

---

### P4.HF11 — Stream compression (investigation, NON-ISSUE — bookkeeping-only commit)

No `cargo add` / `npm install`. No fixtures generated. Prototype source (`add_flate_stream` +
routing in `cos.rs` / `font_embed_cid.rs` / `background.rs`) was written then **reverted**:

```
git checkout -- src-tauri/src/pdf/background.rs \
                src-tauri/src/pdf/cos.rs \
                src-tauri/src/pdf/font_embed_cid.rs
```

Verification that drove the conclusion (measuring the saved artifact's `/FontFile2`):

```
# regenerate an embedded-font artifact with explicit compress() disabled, then inspect it
cargo test -p vibepdf-lib font_embed_cid   # produced the artifact
#   → /FontFile2 … /Length 20893 /Length1 332560 /Filter /FlateDecode
#   i.e. already compressed on save; explicit Compression::best() only reached 20365 B (~2.5%)
```

Post-revert build gate (clean, back to HF10 state):

```
cargo build -p vibepdf-lib     # Finished, no warnings
```

No new test committed (nothing shipped). Docs updated: `FABLE_REVIEW.md` §3.12, `BACKLOG.md`,
`Learning.md`.

---

### P4.HF12 — Dirty-flag correctness (FABLE_REVIEW §3.11)

No `cargo add` / `npm install`; no fixtures generated. Pure Rust change in
`undo.rs` + `actor.rs` + `save_noop.rs` (+ docs).

Verification gates:

```
cargo test --lib undo::                 # 9 passed (state-id unit tests)
cargo test --test save_noop             # 5 passed, 1 ignored (bug a + bug b)
cargo test --test autosave              # 4 passed (touched behaviour unaffected)
npm run check                           # tsc + eslint + clippy -D warnings — green
cargo clippy --fix --lib --tests --allow-dirty --allow-staged   # 2 doc_markdown fixes (FABLE_REVIEW backticks)
npm run test:rust                       # full suite — every 'test result' 0 failed
```

Verification artifact (save path unchanged in bytes, regenerated for the ritual):

```
cargo test --test save_noop save_writes_verification_artifact -- --ignored
#   → /tmp/vibepdf-verify.pdf (693 bytes)
```

---

### P4.HF13 — Undo byte budget (FABLE_REVIEW §3.6)

No `cargo add` / `npm install`; no fixtures; no new dependency. Rust-only change
in `undo.rs` + `restore.rs` (+ docs). No PDF write-path change → no verification
artifact needed.

Verification gates:

```
cargo test --lib undo::                 # 14 passed (9 prior + 5 byte-budget)
cargo clippy --fix --lib --tests --allow-dirty --allow-staged   # doc_markdown (2) — len_zero fixed by hand
npm run check                           # tsc + eslint + clippy -D warnings — green
npm run test:rust                       # full suite — every 'test result' 0 failed
```

---

### P4.HF14 — Strict webview CSP (FABLE_REVIEW §3.8)

No `cargo add` / `npm install`; no new dependency. Config + one guard test (+ docs).

Verification gates:

```
npx vitest run src/__tests__/csp.test.ts   # 2 passed (config-shape guard)
npm run check                              # tsc + eslint + clippy -D warnings — green
npm run build                              # vite prod bundle builds
npm run test                               # 370 passed (89 files) incl. csp.test.ts
python3 -m json.tool src-tauri/tauri.conf.json >/dev/null   # JSON still valid
```

**NOT verifiable here** (no display to drive WKWebView): the CSP is enforced at
runtime, so it needs a manual in-app smoke test — `npm run dev` + a bundled
build, load a normal and a scanned PDF, confirm zero DevTools CSP violations and
working render/thumbnails/text-selection. Deferred to the batch.

---

### P4.HF15 — Windows path fix + CI leg (FABLE_REVIEW §3.9)

No `cargo add` / `npm install`; no new dependency. Frontend TS + a CI workflow
job (+ docs). No Rust source change → no verification artifact.

Verification gates:

```
npx vitest run src/app/__tests__/paths.test.ts   # 6 passed (basename Windows/UNC guard)
npm run check                                    # tsc + eslint + clippy -D warnings — green
npm run test                                     # 376 passed (90 files), was 370
node -e "require('yaml').parse(...ci.yml)"        # workflow parses; 3 jobs incl. check-windows
```

**NOT verifiable here:** the `check-windows` job runs only on GitHub's
`windows-latest`; its first run is the real proof (and may surface latent
Windows-specific clippy/compile issues — that's the point of the leg).

---

### P4.HF16 — Exact base-14 glyph metrics (FABLE_REVIEW §3.10)

No `cargo add` / `npm install`; **no new dependency** (widths measured from the
already-bundled PDFium). One generated data file.

Generate the metrics tables (offline, from bundled PDFium):

```
node scripts/cargo-test.mjs --test gen_font_metrics -- --ignored --nocapture
#   → writes src/pdf/font_metrics/tables.rs (verified Helvetica 'A'=667, space=278, '@'=1015)
```

Verification gates:

```
cargo test --lib font_metrics              # 5 passed (AFM sums, WWW≫iii, Courier=600)
node scripts/cargo-test.mjs --test header_footer --test watermark   # 12 + 13 passed (alignment)
npm run check                              # tsc + eslint + clippy -D warnings — green
cargo clippy --fix --lib --tests --allow-dirty --allow-staged        # doc_markdown (4)
npm run test:rust                          # full suite — every 'test result' 0 failed
```

Verification artifacts regenerated (git-ignored `Sample PDFs/`) for the manual
alignment check: `vibepdf-verify-header-footer.pdf`, `vibepdf-verify-watermark.pdf`.

---

### P4.HF17 — Assorted §3.15 cleanups (5 of 9)

No `cargo add` / `npm install`; no new dependency; no PDF write-path behaviour
change (`parse_hex_color` only *adds* `#rgb`; 6-digit unchanged) → no artifact.

Verification gates:

```
cargo test --lib hex_color   # 3 passed (#rgb == #rrggbb, 6-digit, rejects invalid)
npm run check                # tsc caught a multi-line import missed by grep (free-text-layer), then green
npm run test                 # 376 passed (90 files) incl. the moved free-text.test.ts
npm run test:rust            # full suite — every 'test result' 0 failed
```

---

### P4.HF18 — CID-path unification phase 1 (header/footer)

No `cargo add` / `npm install`; no new dependency (`build_cid_font` already
subsets via the existing `subsetter`/`ttf-parser`). Rust-only.

Verification gates:

```
cargo build --lib                                   # clean
node scripts/cargo-test.mjs --lib header_footer     # 2 passed (render+extract; HF2 tag + hex Tj + cm)
node scripts/cargo-test.mjs --test header_footer    # 12 passed (unchanged behaviour)
cargo clippy --fix --lib --tests --allow-dirty --allow-staged   # if-let-else (1); items_after_statements fixed by hand
npm run check                                       # tsc + eslint + clippy -D warnings — green
npm run test:rust                                   # full suite — every 'test result' 0 failed
```

Verification artifact (embedded Greek/Cyrillic footer, now CID-emitted):

```
node scripts/cargo-test.mjs --test header_footer hf_embedded_unicode_verification_artifact -- --ignored
#   → Sample PDFs/vibepdf-verify-hf-unicode.pdf (git-ignored)
```

---

### P4.HF19 — CID-path unification phase 2 (watermark)

No `cargo add` / `npm install`; no new dependency. Rust-only.

Verification gates:

```
cargo build --lib                                # clean (after dropping the now-dead `base`)
node scripts/cargo-test.mjs --lib watermark      # 5 passed (render+extract; behind; CID+tag+opacity)
node scripts/cargo-test.mjs --test watermark     # 7 passed (unchanged behaviour)
npm run check                                    # tsc + eslint + clippy -D warnings — green
npm run test:rust                                # full suite — every 'test result' 0 failed
```

Verification artifact (embedded Unicode watermark, now CID-emitted):

```
node scripts/cargo-test.mjs --test watermark watermark_embedded_unicode_verification_artifact -- --ignored
#   → Sample PDFs/vibepdf-verify-watermark-unicode.pdf (git-ignored)
```

---

### P4.HF20 — CID-path unification phases 3+4 (text box + delete font_embed)

No `cargo add` / `npm install`. **Removed** dead code: `git rm src-tauri/src/pdf/font_embed.rs`
(the retired PDFium embed path); no dependency change (`subsetter`/`ttf-parser` still used by
`font_embed_cid`).

Verification gates:

```
cargo build --lib                                  # after migrate: font_embed dead; after delete: clean
node scripts/cargo-test.mjs --lib text_box_embed   # 4 passed (wrap+extract; underline rules; CID+tag)
npm run check                                      # tsc + eslint + clippy -D warnings — green
npm run test:rust                                  # full suite — every 'test result' 0 failed
```

Verification artifact (embedded Unicode text box, now CID-emitted):

```
node scripts/cargo-test.mjs --test text_box text_box_embedded_unicode_artifact -- --ignored
#   → Sample PDFs/vibepdf-verify-text-box-unicode.pdf (git-ignored)
```

Docs: font_embed_cid module doc + docs/04 rewritten for the single backend; two stale
`[crate::pdf::font_embed]` intra-doc links repointed to `font_embed_cid`.

---

## P4.HF21 — Text-tool disambiguation (Phase 0 of re-editable Add Text)

UI-only relabel of the two text tools in `MarkupToolbar.tsx` ("Text" → "Text Box";
clarify "Add Text" tooltips). No Rust, no PDF write path, no new command/dep.

Verification gates (frontend only — cargo untouched, so clippy N/A):

```
npx tsc --noEmit                                   # OK
npx eslint src --max-warnings=0                    # OK
npx vitest run                                     # 90 files / 376 tests, 0 failed
```

No verification-PDF artifact (no write path changed). Phase 1–3 (the actual
re-edit feature) is blocked on a new spec line and is not in this commit.

---

## P4.HF22 — Re-editable Add Text, Phase 1 (emit metadata + read)

Rust-only (cos.rs emit + read, font_embed_cid.rs fragment split, spec line,
artifact test). No new deps, no new command yet (Phases 2–3).

Verification gates:

```
cd src-tauri && cargo clippy --all-targets -- -D warnings   # clean (after doc_markdown + &str + too_many_args fixes)
node scripts/cargo-test.mjs --lib text_box                  # 8 passed (4 new round-trip tests)
npm run test:rust                                           # full suite, every 'test result' 0 failed
npm run check                                               # tsc + eslint + clippy all clean
```

Verification artifacts (both regenerated / added, git-ignored):

```
node scripts/cargo-test.mjs --test text_box -- --ignored
#   → Sample PDFs/vibepdf-verify-text-box-unicode.pdf   (CID box, now one tag)
#   → Sample PDFs/vibepdf-verify-text-box-ascii.pdf      (NEW: WinAnsi box, now BDC-wrapped)
```

---

## P4.HF23 — Re-editable Add Text, Phase 2 (splice + update + actor + IPC)

Rust-only (cos remove/update, annotation Edit, actor messages, 2 commands +
register, actor round-trip test). No new deps, no frontend yet (Phase 3).

Verification gates:

```
cd src-tauri && cargo clippy --all-targets -- -D warnings   # clean
node scripts/cargo-test.mjs --lib text_box                  # 11 passed (3 new: remove/update)
node scripts/cargo-test.mjs --test text_box                 # actor round-trip passes (2 artifacts still ignored)
npm run test:rust                                           # full suite, 0 FAILED / 0 panicked
npx tsc --noEmit && npx eslint src --max-warnings=0          # clean (frontend untouched)
```

No new verification artifact: `update_text_box` = remove + `add_text_box`, whose
rendered output is already covered by the Phase-1 `text-box-{ascii,unicode}` artifacts.

---

## P4.HF24 — Re-editable Add Text, Phase 3 (frontend re-edit layer)

Frontend-only (IPC wrappers + text-box-layer re-edit + tests). No Rust delta, no
new dep. Completes P4-EDIT-003b end-to-end.

Verification gates:

```
npx tsc --noEmit                                                    # OK
npx eslint src --max-warnings=0                                     # OK
npx vitest run src/view/__tests__/text-box-layer.test.tsx          # 4 passed (2 new: pass-through + re-edit)
npx vitest run                                                      # 90 files / 377 tests, 0 failed
```

`cargo clippy` unchanged (no Rust touched this phase; last green in P4.HF23).
In-app verification (double-click an added box → edit → Save) deferred to the
user — the browser tools can't drive the native Tauri window.

---

## P4.HF25 Step 1 — Delete-a-text-box primitive + command

Rust delete edit + actor message + command + register; `deleteTextBox` IPC. No new
dep, no new PDF logic (wraps the existing `remove_text_box`).

Verification gates:

```
cd src-tauri && cargo clippy --all-targets -- -D warnings              # clean
node scripts/cargo-test.mjs --test text_box text_box_delete_roundtrip_through_actor  # ok
npx tsc --noEmit && npx eslint src --max-warnings=0                     # clean
```

---

## P4.HF25 Step 2 — Empty-edit deletes + re-edit via Edit Text

Frontend-only (two layers + their tests). No Rust delta, no new dep.

Verification gates:

```
npx tsc --noEmit                                                     # OK
npx eslint src --max-warnings=0                                      # OK
npx vitest run src/view/__tests__/text-{box,edit}-layer.test.tsx     # 11 passed
```

---

## P4.HF25 Step 3 — Text gets its own colour (black default)

Frontend-only (types + store + 2 layers + toolbar + free-text test). No Rust delta,
no new dep. `textColor` is optional (default `#000000`).

Verification gates:

```
npx tsc --noEmit                        # OK
npx eslint src --max-warnings=0         # OK
npx vitest run                          # 90 files / 379 tests, 0 failed
```

---

## P4.HF26 (item 1) — Edit-Image drag/resize origin fix

Frontend-only (one layer). No Rust delta, no new dep.

```
npx tsc --noEmit                                              # OK
npx eslint src/view/image-edit-layer.tsx --max-warnings=0    # OK
npx vitest run src/view/__tests__/image-edit-layer.test.tsx  # 5 passed
```

Coordinate/layout fix — jsdom returns zero rects, so in-app is the real check.

---

## P4.HF26 (item 2) — Remove watermarks

Rust: `clear_decorations` (cos) + `RemoveWatermarksEdit` (watermark) + actor msg +
command + register. Frontend: `removeWatermarks` IPC + dialog button. No new dep.

Verification gates:

```
cd src-tauri && cargo clippy --all-targets -- -D warnings   # clean (after VibePDF backtick fix)
node scripts/cargo-test.mjs --lib watermark                 # 7 passed (2 new clear_decorations)
node scripts/cargo-test.mjs --test watermark                # 14 passed (1 new actor remove/undo)
npm run check                                               # tsc + eslint + clippy clean
```

---

## P4.HF26 (item 3) — Image background: Adobe CMYK JPEG /Decode fix

Rust only (image_xobject.rs encoder fix + a background render regression test).
No new dep. Fix lands across every image feature (add/stamp/watermark/background).

Verification gates:

```
node scripts/cargo-test.mjs --test background image_background_actually_renders  # proves the path renders
node scripts/cargo-test.mjs --lib image_xobject     # 10 passed (2 new: Adobe-marker + CMYK Decode)
node scripts/cargo-test.mjs --test background        # 18 passed
cd src-tauri && cargo clippy --all-targets -- -D warnings   # clean
npx tsc --noEmit                                     # OK (no frontend change)
```

---

## P4.HF27 — Background replaces instead of stacking behind

Rust only (cos: extract `remove_decorations_on_page`; background: evict-before-add;
render regression test). No new dep, no frontend change.

Verification gates:

```
node scripts/cargo-test.mjs --test background background_replaces_previous_not_stacks  # ok
node scripts/cargo-test.mjs --test background        # 19 passed
node scripts/cargo-test.mjs --lib watermark          # 7 passed (clear_decorations refactor safe)
cd src-tauri && cargo clippy --all-targets -- -D warnings   # clean
```

---

## P4.HF28 — Edit-preview reload returns raw bytes (big-PDF fix)

`pdf_get_bytes` → `tauri::ipc::Response` (raw ArrayBuffer) instead of `Vec<u8>`
(JSON `number[]`); `getPdfBytes` decodes the buffer. No new dep. Root-caused via
temporary actor/command tracing (reverted before commit).

Verification gates:

```
npm run check                                            # tsc + eslint + clippy clean
npx vitest run src/ipc/__tests__/pdf-get-bytes.test.ts   # 3 passed
node scripts/cargo-test.mjs --test get_bytes             # 2 passed (actor path unchanged)
npx vitest run src/ipc                                   # 68 passed (30 files)
```

---

## P4.HF29 — Optimistic edit preview (ink + text box)

Frontend only. New `optimistic-edit-store` + wiring in `ink-layer`,
`text-box-layer`, `PdfViewer`. No new dep, no Rust change.

Verification gates:

```
npx vitest run src/state/__tests__/optimistic-edit-store.test.ts   # 8 passed
npm run check                                                       # tsc + eslint + clippy clean
npx vitest run src/state src/ipc src/view                          # 196 passed (56 files)
```

---

## P4.PERF1 — Read cache (stop re-parsing the whole PDF per query)

Rust only (`doc_cache.rs` + cos read-fn split + actor wiring). No new dep, no
frontend change, no write-path change (byte-identical output).

Verification gates:

```
npm run check                                                    # tsc + eslint + clippy pedantic clean
node scripts/cargo-test.mjs --lib doc_cache                      # 1 passed (cache unit test)
node scripts/cargo-test.mjs --test doc_cache                     # 1 passed (actor invalidate on edit + undo)
node scripts/cargo-test.mjs --lib                                # 91 passed
node scripts/cargo-test.mjs --test text_box --test free_text --test measure \
  --test read_annotations --test text_note                       # all green (reads via cache)
```

---

## P4.D4 — Page numbers (SPEC P4-EDIT-011)

New `pdf/page_numbers.rs` + actor message/command/register; frontend ipc wrapper
+ `PageNumbersDialog` + toolbar button. New dep: none. `docs/02` unchanged.

Verification gates + tests:

```
npm run check                                                   # tsc + eslint + clippy pedantic clean
cargo test --test page_numbers                                  # 12 passed, 1 ignored (artifact)
cargo test --lib page_numbers                                   # 5 passed (format_number: roman/alpha/composites/parse)
npx vitest run src/app/__tests__/PageNumbersDialog.test.tsx     # 6 passed
npx vitest run src/app src/tools/__tests__/page-range.test.ts src/view/__tests__  # 135 passed (incl. ZoomToolbar)
cargo test --lib --test header_footer --test watermark --test background  # 96 + siblings green (no regression)
cargo test --test page_numbers pn_writes_verification_artifact -- --ignored  # wrote Sample PDFs/vibepdf-verify-page-numbers.pdf
```

Two clippy fixes on my own additions: `const` moved above the guard in
`to_roman` (`items_after_statements`); `#[allow(clippy::too_many_lines)]` on
`lib::run()` (the command list crossed 100 lines by one).

---

## P4.D5 — Bates numbering (SPEC P4-EDIT-012)

New `pdf/bates.rs` + actor message/command/register; frontend ipc wrapper +
`BatesDialog` + toolbar button. New dep: none. `docs/02` unchanged. Single-doc;
cross-document batch deferred (merge-then-Bates covers it).

Verification gates + tests:

```
npm run check                                                   # tsc + eslint + clippy pedantic clean
cargo test --test bates                                         # 10 passed, 1 ignored (artifact)
cargo test --lib bates                                          # 4 passed (bates_label: pad/overflow/prefix-suffix)
npx vitest run src/app/__tests__/BatesDialog.test.tsx           # 5 passed
npx vitest run src/app                                          # 47 passed (incl. ZoomToolbar + PageNumbers)
cargo test --lib --test page_numbers --test header_footer --test watermark  # 100 lib + siblings green (no regression)
cargo test --test bates bates_writes_verification_artifact -- --ignored     # wrote Sample PDFs/vibepdf-verify-bates.pdf
```

One clippy fix on my own code: `uninlined_format_args` in `bates_label`
(`{value:0>padding$}` instead of `width = padding`). One test-design fix:
`BatesDialog` empty-field validation now checks `trim() === ""` (empty → 0 is a
*valid* Bates start, so the guard had to key off the cleared string to fire).

---

## P4.PERF4 — deferring the bake (reload/edit-preview UX)

Frontend only: two-epoch soft/hard reload model + idle backstop, freeze-frame,
tab-switch fix, plus edit-text no-run toast + instant preview. No dep, no backend
change, `docs/02` unchanged.

Verification gates + tests:

```
npm run check                                                   # tsc + eslint + clippy pedantic clean
npx tsc --noEmit                                                # boundary check mid-way (store + PdfViewer)
npx vitest run src/state/__tests__/edit-epoch-store.test.ts     # 11 passed (soft/hard, switch-snap, pendingBake lifecycle)
npm run test                                                    # 408 passed / 94 files (no regressions)
```

Bugs found + fixed during in-app testing:
- Idle backstop re-fired forever (compared raw vs bake — independent counters).
  Replaced with a `pendingBake` flag cleared by any bake → fires exactly once.
- Tab switch fired a spurious 2nd reload (debounced epoch didn't snap on id change).
- clippy `uninlined_format_args`, `items_after_statements`; eslint `getComputedStyle`
  global (used `window.getComputedStyle`); `exactOptionalPropertyTypes` on an
  optional callback prop.

Debounce: main-view reload now keys off the *bake* epoch (400ms); the bake epoch
only advances on hard edits / the 8s idle backstop, so bursts of soft edits cause
no reload.

---

## P5.A1 — Detect AcroForm + Form-mode entry point (SPEC P5-FORM-001)

No `npm install` / `cargo add` — no new dependency (reused `lopdf`).

Verification gates + tests:

```
cargo test --lib pdf::form                        # 5 ok (walk semantics: radio→1, hierarchy→2, XFA)
cargo test --test form_detect                     # 3 ok (forms.pdf via bytes + actor; no-form → 0)
cargo test --lib pdf::cos                          # 20 ok (acroform_dict refactor)
cargo test --test merge -- --test-threads=1        # 7 ok (field-rename path intact)
npm run check                                      # tsc + eslint + clippy pedantic clean
npm run test                                       # 96 files / 414 passed
npm run test:rust                                  # every binary 0 failed
```

Note: `cargo test --test merge` under default parallelism SIGSEGVs inside PDFium
(known thread-unsafety — the `merge_carries_form_fields_with_rename` test that
exercises this change passes; the crash is a later PDFium test running
concurrently). The project runner serialises PDFium tests.

---

## P5.A2 — Fill text fields (SPEC P5-FORM-002)

No `npm install` / `cargo add` — no new dependency (reused `lopdf`).

New fixture: `tests/fixtures/basic/forms-multi.pdf` via
`python3 tests/fixtures/basic/generate-forms-multi.py` (3 text fields: plain,
`/MaxLen 5`, multiline).

Verification gates + tests:

```
cargo test --test form_fill_text                   # 7 ok (geometry, fill, max-len, unicode, NeedAppearances, actor fill→undo)
npm run check                                       # tsc + eslint + clippy pedantic clean
npm run test                                        # 97 files / 420 passed
npm run test:rust                                   # every binary 0 failed
cargo test --test form_fill_text writes_verification_artifact -- --ignored   # → $TMPDIR/vibepdf-verify.pdf (cross-reader)
```

Write path → a verification artifact was produced for the cross-reader ritual
(copied to `Sample PDFs/vibepdf-verify-formfill.pdf`, git-ignored). On macOS
`std::env::temp_dir()` is `$TMPDIR` (`/var/folders/…`), not `/tmp`.

---

## P5.A3 — Fill checkbox / radio (SPEC P5-FORM-003)

No `npm install` / `cargo add` — no new dependency (reused `lopdf`).

New fixture: `tests/fixtures/basic/forms-buttons.pdf` via
`python3 tests/fixtures/basic/generate-forms-buttons.py` (checkbox `agree` +
radio group `color` with `/Red`,`/Green`; real `/AP /N` on/off appearances).

Verification gates + tests:

```
cargo test --test form_fill_checkbox                 # 6 ok (read kind/on-state, check/uncheck, radio sibling /AS flip, non-Yes state, NeedAppearances untouched, actor set→undo)
npm run check                                         # tsc + eslint + clippy pedantic clean
npm run test                                          # 98 files / 426 passed
npm run test:rust                                     # every binary 0 failed
cargo test --test form_fill_checkbox writes_verification_artifact -- --ignored   # → $TMPDIR/vibepdf-verify.pdf
```

Write path → verification artifact copied to `Sample PDFs/vibepdf-verify-formbuttons.pdf`
(git-ignored) for the cross-reader ritual.

---

## P5.A4 — Fill choice fields (combo, list) (SPEC P5-FORM-004)

No `npm install` / `cargo add` — no new dependency (reused `lopdf`).

New fixture: `tests/fixtures/basic/forms-choice.pdf` via
`python3 tests/fixtures/basic/generate-forms-choice.py` (single combo `fruit`
with a labelled `[chy Cherry]` option + multi-select list `colors`).

Verification gates + tests:

```
cargo test --test form_fill_choice                   # 7 ok (read options/kind/multi/selection, labelled export≠display, combo select, multi list /V array + /I, reject unknown, NeedAppearances, actor set→undo)
npm run check                                         # tsc + eslint + clippy pedantic clean
npm run test                                          # 99 files / 431 passed
npm run test:rust                                     # every binary 0 failed
cargo test --test form_fill_choice writes_verification_artifact -- --ignored   # → $TMPDIR/vibepdf-verify.pdf
```

Write path → verification artifact copied to `Sample PDFs/vibepdf-verify-formchoice.pdf`
(git-ignored) for the cross-reader ritual.

---

## P5.A5 — XFA degraded support (SPEC P5-FORM-005)

No `npm install` / `cargo add` — no new dependency (reused `lopdf`).

New fixture: `tests/fixtures/basic/forms-xfa.pdf` via
`python3 tests/fixtures/basic/generate-forms-xfa.py` (XFA-only: empty AcroForm
`/Fields` + an `/XFA` stream + static page text).

Verification gates + tests:

```
cargo test --test form_xfa_degraded                  # 5 ok (detect XFA-only, strip removes /XFA, NeedAppearances, no-XFA errors, actor strip→undo)
npm run check                                         # tsc + eslint + clippy pedantic clean
npm run test                                          # 100 files / 436 passed
npm run test:rust                                     # every binary 0 failed
cargo test --test form_xfa_degraded writes_verification_artifact -- --ignored   # → $TMPDIR/vibepdf-verify.pdf
```

Write path → verification artifact copied to `Sample PDFs/vibepdf-verify-xfa-flattened.pdf`
(git-ignored) for the cross-reader ritual. **Closes Track A of Phase 5.**

---

## P5.B1 — Create text field (SPEC P5-FORM-006)

No `npm install` / `cargo add` — no new dependency (reused `lopdf`). No new
fixture (uses `hello.pdf` for the form-less path + `forms.pdf` for the existing-form path).

Verification gates + tests:

```
cargo test --test form_create_text                   # 8 ok (create, AcroForm-when-absent, default+flags, dup reject, NeedAppearances, add-into-existing, empty-name reject, actor create→undo)
npm run check                                         # tsc + eslint + clippy pedantic clean
npm run test                                          # 101 files / 440 passed
npm run test:rust                                     # every binary 0 failed
cargo test --test form_create_text writes_verification_artifact -- --ignored   # → $TMPDIR/vibepdf-verify.pdf
```

Write path → verification artifact copied to `Sample PDFs/vibepdf-verify-createfield.pdf`
(git-ignored) for the cross-reader ritual. **Opens Track B (form authoring).**

---

## P5.B2 — Create other field kinds (SPEC P5-FORM-007)

No `npm install` / `cargo add` — no new dependency. No new fixture (uses
`hello.pdf` + `forms.pdf`).

Verification gates + tests:

```
cargo test --test form_create_other                  # 9 ok (checkbox, radio group (3 kids), radio<2 reject, combo, list multi, signature /FT /Sig, pushbutton /Ff bit + excluded from button read, dup reject, actor create→undo)
npm run check                                         # tsc + eslint + clippy pedantic clean
npm run test                                          # 101 files / 443 passed
npm run test:rust                                     # every binary 0 failed
cargo test --test form_create_other writes_verification_artifact -- --ignored   # → $TMPDIR/vibepdf-verify.pdf
```

Write path → verification artifact copied to `Sample PDFs/vibepdf-verify-createfields.pdf`
(git-ignored) — checkbox + radio + combo + push-button on one page.

---

## P5.B3 — Tab order + field property editor (SPEC P5-FORM-006b/006c, drafted)

No `npm install` / `cargo add` — no new dependency. No new fixture (builds test
docs from `hello.pdf` via the B1/B2 create paths).

**Spec note:** B3 had no spec line; drafted P5-FORM-006b (edit properties) and
P5-FORM-006c (tab order) and implemented against them — they still need adding to
`docs/02_PRODUCT_SPEC.md` (human-owned).

Verification gates + tests:

```
cargo test --test form_properties                    # 11 ok (list in tab order, rename, rename collision, value/maxlen/flags/tooltip, clear maxlen+tooltip, unknown field, tab order + /Tabs /S, unlisted-kept, delete, delete radio+kids, actor edit→undo)
npx vitest run src/tools/form-author/__tests__/tab-order.test.ts   # 8 ok (moveItem/up/down, clamping, immutability)
npm run check                                         # tsc + eslint + clippy pedantic clean (first try)
npm run test                                          # 103 files / 461 passed
npm run test:rust                                     # every binary 0 failed
cargo test --test form_properties writes_verification_artifact -- --ignored   # → $TMPDIR/vibepdf-verify.pdf
```

Design note: the `max_len` wire is a value + `clear_max_len` flag, because
`Option<Option<u32>>` cannot round-trip through JSON (caught before shipping).

Write path → verification artifact copied to `Sample PDFs/vibepdf-verify-fieldprops.pdf`
(git-ignored) — renamed + required + tooltipped field with a reordered tab sequence.

---

## P5.C1 — Export form data (FDF / XFDF / JSON / CSV) (SPEC P5-FORM-008)

No `npm install` / `cargo add` — `serde_json` was already a dependency. No new
fixture (builds a mixed form from `hello.pdf` via the B1/B2 create paths).

Verification gates + tests:

```
cargo test --test form_export                        # 10 ok (collect all kinds + pushbutton excluded, signature empty, FDF parses as PDF syntax, XFDF escaping + repeated <value>, JSON round-trip, CSV header + join, unicode, no-form, bad format, actor export)
npm run check                                         # tsc + eslint + clippy pedantic clean (after 6 nit fixes)
npm run test                                          # 463 passed
npm run test:rust                                     # every binary 0 failed
cargo test --test form_export writes_verification_artifacts -- --ignored   # → $TMPDIR/vibepdf-verify-formdata.{fdf,xfdf,json,csv}
```

Two things caught mid-flight: (1) the FDF test failed with
`Parse(InvalidFileHeader)` — lopdf's loader requires `%PDF-`, so the test now
swaps the header back to verify the *body*; the product's `%FDF-1.2` header is
correct. (2) clippy pedantic flagged 6 nits (redundant closure, two missing
`#[must_use]`, three `format!`-append) — fixed with `writeln!`.

All four artifacts copied to `Sample PDFs/vibepdf-verify-formdata.*` (git-ignored).

---

## P5.C2 — Import form data + Flatten (SPEC P5-FORM-009, P5-FORM-010)

No `npm install` / `cargo add` — every parser is hand-rolled or uses the existing
`serde_json`. No new fixture (both suites build their forms from `hello.pdf` via
the B1/B2 create paths; the XFA case reuses `tests/fixtures/basic/forms-xfa.pdf`).

Verification gates + tests:

```
cargo check --all-targets                            # after each increment (refactor / new modules / actor / commands)
cargo test --test form_import --test form_flatten    # 14 + 14 ok
npx tsc --noEmit -p tsconfig.json                    # frontend typecheck after the IPC + panel wiring
npx vitest run src/app/__tests__/FieldPropertiesPanel.test.tsx src/tools/form-author/__tests__/import-report.test.ts   # 18 passed
npm run check                                         # tsc + eslint + clippy pedantic (one doc_markdown fix: XObject → `XObject`)
npm run test                                          # 104 files, 475 passed
npm run test:rust                                     # 73 binaries, every one 0 failed
cargo test --test form_import --test form_flatten -- --ignored --nocapture   # → $TMPDIR/vibepdf-verify-form-{import,flatten}.pdf
```

Artifact sanity check (no pdftotext/mutool/qpdf on this box, so a raw-bytes probe):

```
python3 -c "d=open('Sample PDFs/vibepdf-verify-form-flatten.pdf','rb').read(); print(b'/AcroForm' in d, b'/Widget' in d, b'Ada Lovelace' in d)"   # False False True
python3 -c "d=open('Sample PDFs/vibepdf-verify-form-import.pdf','rb').read(); print(b'/AcroForm' in d, d.count(b'/Widget'), b'Ada' in d)"        # True 6 True
```

Both artifacts copied to `Sample PDFs/` (git-ignored); the flatten one also to
`/tmp/vibepdf-verify.pdf` for the cross-reader ritual.

Only real course-correction: the plan said flatten would skip push-buttons, but a
push-button's face *is* its current appearance, so the generic bake handles it and
no special case was written. Hidden widgets (`/F` bit 2) are the one thing removed
without being drawn.

---

## P5 sweep fixes — ten defects from the first in-app pass

No `npm install` / `cargo add`. No new fixture; the sweep assets are rebuilt by
the committed `scripts/generate-sweep-form.py`.

Investigation (how each verdict was reached rather than guessed):

```
grep -n "NeedAppearances" node_modules/pdfjs-dist/build/pdf.worker.mjs   # → line 53085 proves PDF.js synthesizes the appearance from /V
grep -n "pub struct FormField" -A 22 src-tauri/src/pdf/form.rs           # → no tooltip field; /TU never reached the fill overlay
sed -n '/fn pushbutton_appearance/,/^}/p' src-tauri/src/pdf/form.rs      # → grey box + border, caption never drawn
grep -n "readFormSummary\|setDetected" src/app/use-form-detect.ts        # → keyed on documentId only
```

Verification gates + tests:

```
cargo check --all-targets                             # after each increment
cargo test --test form_create_other                   # 18 ok (8 new: radio square + curves, combo default reject/keep, list grow/no-shrink, signature /AP, caption drawn, nested duplicate name)
npx vitest run src/view/__tests__/form-choices-layer.test.tsx   # 6 ok (3 new)
npx vitest run src/view/__tests__/form-fields-layer.test.tsx    # 7 ok (3 new)
npx vitest run src/app/__tests__/use-form-detect.test.tsx       # 5 ok (new file)
npm run check                                          # clean after 5 clippy nits (redundant closure, excessive float precision, format!-append, doc_markdown, for_kv_map)
npm run test                                           # 105 files, 486 passed
npm run test:rust                                      # 73 binaries, every one 0 failed
cargo test --test form_create_other --test form_flatten --test form_import writes_verification -- --ignored --nocapture
```

Artifacts refreshed in `Sample PDFs/verify/p5-forms/` (createfields now covers
all six field kinds, not four). Byte probe on the new createfields artifact:

```
python3 -c "d=open('Sample PDFs/verify/p5-forms/vibepdf-verify-createfields.pdf','rb').read(); print(b'(Submit) Tj' in d, b'/Sig' in d, b'[3 2] 0 d' in d)"   # True True True
```

The grown list-box rect could NOT be confirmed from the artifact bytes — lopdf
compresses objects into object streams — so that one rests on
`list_box_grows_to_fit_its_options`, which asserts it directly.

Two self-inflicted stumbles worth recording: a bulk regex that added `tooltip:
null` to every `multi:`/`multiline:` literal also hit two `pdf_add_field` **wire
payload** expectations (addField sends no tooltip) — caught by the full suite,
not by the targeted runs. And `npm run dev` exited when `cargo check` collided
with the Tauri file watcher; run one or the other.

---

## Sweep round two — layout + five form fixes (be21bdd → f426fda)

Five commits, no bookkeeping entry until now; this covers all of them.
No `npm install` / `cargo add`. No new fixture — `scripts/generate-sweep-form.py`
changed (round radio `/AP` faces), so **regenerate before re-running the sweep**:

```
python3 scripts/generate-sweep-form.py
```

Investigation:

```
grep -n "NeedAppearances" node_modules/pdfjs-dist/build/pdf.worker.mjs      # 53085 — PDF.js synthesizes from /V
grep -o '<aside className="[^"]*"' src/panels/*.tsx                          # only one panel had shrink-0
grep -rn "bumpBake\|useHasPendingBake" src/view/PdfViewer.tsx                # IDLE_BAKE_MS = 8000
```

The centering bug was **measured**, not reasoned about — a probe page served to
a real browser, 700px child in a 400px scroller:

```
before  clippedLeft 148px  scrollWidth 550   (page is 700 — 148px uncounted)
after   clippedLeft 0      scrollWidth 700   maxScrollLeft 300
```

Getting a browser onto it took three attempts and is worth recording:
`preview_start` renders `file://` outside the project as a static snapshot (no
JS); the Browser pane blocks `localhost` by policy; a file written **inside** the
project and opened via the Browser pane does run scripts. Also confirmed the
Tailwind class actually lands, because a class that silently doesn't exist is a
no-op that still passes every test:

```
npm run build && grep -ro "min-width:min-content" dist/assets/*.css   # found
rm -rf dist
```

Gates:

```
npm run check                        # clean (5 clippy nits fixed along the way)
npm run test                         # 502 passed
cargo test --test form_create_other  # 20 ok
```

Two traps hit while doing this, both self-inflicted:

- A `curl` loop reported "vite serving" — port 5199 was held by an unrelated
  project's Vite, which answers any path. The check passed while measuring
  nothing. Don't health-check a port you didn't bind.
- `git add -A` staged the local-only `docs/PORTFOLIO_*.md` and `docs/story/`.
  Unstaged and added to `.gitignore` so it can't recur.

`npm run dev` and `cargo check` still cannot run at the same time — the Tauri
watcher exits. Run one or the other.

---

## P6.A1 — Signature library infrastructure (no spec line; supports P6-SEC-001/-002/-003)

No `npm install` / `cargo add` — `uuid`, `serde`, `serde_json` were already in
use. No fixture: the tests build their own PNGs and run against a temp dir that
is removed on drop, so nothing touches `app_data_dir`.

Verification gates + tests:

```
cargo check --all-targets                    # after the module + command wiring
cargo test --test signature_library          # 12 ok
npx vitest run src/ipc/__tests__/signatures.test.ts        # 4 ok
npx vitest run src/state/__tests__/signature-store.test.ts # 4 ok
npm run check                                # clean first time
npm run test                                 # 510 passed
npm run test:rust                            # 74 binaries (was 73), every one 0 failed
```

No PDF write path in this step — the library never opens a document — so there
is **no cross-reader verification artifact** to produce.

One stumble worth recording: the `docs/04_ARCHITECTURE.md` edit was scripted as
two `str.replace` calls followed by one write, and the second anchor did not
match. Because the exception fired before the write, the *first* replacement was
silently discarded too — the file was left untouched while `npm run check`
(running after the `;`) passed and made it look like the edit had landed. Assert
each anchor before mutating, or write after each step.

---

## P6.A2 — Draw signature (SPEC P6-SEC-001)

No `npm install` / `cargo add`, and **no Rust changes at all** — A1's
`signatures_add` already took PNG bytes, so this step is frontend-only. No new
IPC command. No fixture (nothing opens a PDF).

Verification gates + tests:

```
npx vitest run src/tools/signature/__tests__/draw.test.ts        # 11 ok
npx vitest run src/app/__tests__/SignatureDialog.test.tsx        # 8 ok
npx vitest run src/view/__tests__/render-page.test.ts            # re-run after the getContext stub — still ok
npm run check                                                    # clean
npm run test                                                     # 110 files, 529 passed
```

`npm run test:rust` not re-run: no Rust file was touched this step.

**Not covered by any of the above:** `src/tools/signature/raster.ts`. jsdom has
no canvas, so the rasteriser cannot execute under vitest. Its correctness rests
on the acceptance check — open the stored PNG from
`~/Library/Application Support/dev.vibepdf/signatures/` and confirm a
transparent background and a tight crop.

One test was wrong before the code was: the "pads without clipping" assertion
used `toBeCloseTo` (0.005 tolerance) against a canvas size that is rounded to
whole pixels, so it demanded fractional canvases. Relaxed to ±0.5px with the
reason in a comment.

---

## P6.A3 — Type signature (SPEC P6-SEC-002)

No `npm install` / `cargo add`, no Rust changes, no new IPC — A1's
`signatures_add` still takes the PNG. **No fonts bundled**: Option A (system
fonts, detected at runtime) was the approved route, so nothing was downloaded.

Verification gates + tests:

```
npx vitest run src/tools/signature/__tests__/fonts.test.ts    # 9 ok
npx vitest run src/tools/signature/__tests__/raster.test.ts   # 18 ok (9 new for textToPng)
npx vitest run src/app/__tests__/SignatureDialog.test.tsx     # 14 ok (6 new for Type mode)
npm run check                                                 # clean after one exactOptionalPropertyTypes fix
npm run test                                                  # 563 passed
```

`npm run test:rust` not re-run — no Rust file touched.

Two things worth remembering:

- `exactOptionalPropertyTypes: true` rejects `{ family: undefined }`. Omit the
  key instead: `family ? { family } : {}`.
- Adding the "type" mode button broke an existing assertion —
  `getByText("type")` had been unambiguous and now matched both the button and a
  library badge. Fixed by scoping to the list via `within(getByLabelText("Saved
  signatures"))`, and by giving the list the label it should always have had.
  The test was too loose from the start; the new UI only exposed it.

Not covered by any test, unchanged from A2: that a real 2D context turns the
recorded draw calls into the expected pixels. Decoding a saved PNG confirms it —
the helper used for A2 works on typed signatures too.

---

## P6.A4 — Image signature (SPEC P6-SEC-003)

**No `npm install`, no `cargo add`, no Rust changes, no new IPC.** That was the
decision of the step: the Rust side deliberately cannot decode JPEG or BMP
(`Cargo.toml` takes `png` alone, with the reason written beside it), so doing
this there would have meant adding the `image` crate to duplicate a decoder the
WebView already ships. `@tauri-apps/plugin-fs` and `@tauri-apps/plugin-dialog`
were already dependencies.

Verification gates + tests:

```
npx vitest run src/tools/signature src/app/__tests__/SignatureDialog.test.tsx   # 96 ok
npm run check                                                                   # clean after 4 eslint globals
npm run test                                                                    # 607 passed (114 files)
npm run test:rust                                                               # green — unchanged, run to confirm
```

New: `threshold.test.ts` 23; `raster.test.ts` 18 → 29; `SignatureDialog.test.tsx` 14 → 24.

Mutation checks — the new assertions were confirmed to bite before being
trusted, since all 96 passed on the first run:

```
# 1. opaqueBounds inclusive extents -> exclusive   => 7 failed
# 2. applyThreshold cutoff >= -> >                 => 2 failed
# 3. dialog stops passing `strength` to imageToPng => 1 failed
```

Each was applied with `perl -0pi -e`, run, and reverted from a `/tmp` copy. All
three were caught by the test that was supposed to catch them, and by no others.

`npm run check` needed four additions to the `globals` allowlist in
`eslint.config.js`: `btoa`, `BlobPart`, `CanvasImageSource`, `createImageBitmap`.
That list is hand-maintained; new browser APIs always cost one entry.

Not covered by any test: that WebKit decodes BMP at all. `createImageBitmap`
sniffs the bytes and BMP is the least-exercised of the three formats the spec
names — it is called out in the acceptance check for exactly that reason. Also
unchanged from A2/A3: that a real 2D context produces the expected pixels from
the recorded calls.

---

## P6.A5a — Place signature as a stamp (SPEC P6-SEC-004, first clause)

No `npm install`, no `cargo add`, **no new PDF primitive**. `ImageStampEdit`
(P3.C3b) already embeds a PNG with its alpha as an `/SMask` and returns an
inverse, so the backend change is one command that resolves a library id to
bytes and calls it.

**A5b is not built.** The spec line's second clause — a PKCS#7 signature into a
`/Sig` field — needs P6.B1. `steps/P6.md:113` orders A5 after B1 and B2, which
is why the step splits. Clicking a signature field declines rather than stamping
over it.

Fixture generation (both git-ignored, under `Sample PDFs/signatures/`):

```
python3 gen_sigfield_pdf.py      # a hand-written PDF with 2 /Sig fields + 1 /Tx
qlmanage -t -s 400 -o "$T" "Sample PDFs/signatures/sigfield-form.pdf"
```

The PDF is hand-written on purpose: one produced by our own P5.B2 field writer
would only prove the code agrees with itself. `qlmanage` renders it through
CoreGraphics — a parser sharing no code with ours — and it came out correct.

Verification gates + tests:

```
cargo test --test signature_place                       # 4 ok
cargo test --test signature_place -- --include-ignored  # 5 ok (writes the artifact)
npm run check                                           # clean after one TS annotation fix
npm run test                                            # 624 passed (115 files)
npm run test:rust                                       # green
```

Artifact for the cross-reader ritual: `Sample PDFs/vibepdf-verify-signature.pdf`
(two placements, one at full and one at half opacity).

Mutation checks on the safety-critical guard:

```
# A. signatureFieldAt stops checking /FT      => 1 failed (ignores other kinds)
# B. the decline reports but places anyway    => 1 failed (writes nothing)
```

Each failed only the test meant to catch it.

**One config fix, and it mattered:** `npm run test` was silently collecting a
second copy of every test file from `.claude/worktrees/`, an agent worktree that
is a full checkout. The `@` alias still resolved to the main `src`, so stale
tests ran against new source and produced unhandled rejections for reasons that
no longer existed. `vite.config.ts` now says where tests live:

```ts
include: ["src/**/*.{test,spec}.{ts,tsx}"],
```

For the record, P6.A4's reported 607 was **not** affected — the worktree was
created after that run, at A4's own commit.

**Found in passing, not fixed here:** `add_image_stamp` clamps an over-wide
image by truncating its width alone, so a 1200×40 source comes back 612×40 —
squashed, contradicting its own "never stretched". Pre-existing P3 code. Pinned
by `an_extremely_wide_signature_is_clamped_to_the_page_at_the_cost_of_its_aspect`,
whose name and comment say it records the behaviour rather than endorsing it,
and raised as separate work.

Not covered by any test: that the placed signature renders correctly in Acrobat,
Preview and a third reader. That is the human ritual, and transparency is
exactly the sort of thing one renderer gets right and another does not.

### Follow-up — two flow bugs found in the first minute of in-app use

Reported: the stamp tab opens with its palette when you place a signature, and
the tool stays armed afterwards. Both mine, both from reusing the stamp flow
too faithfully. No Rust change.

- New `"signature"` ToolId, so the rubber-stamp palette and the pressed Stamp
  button stay out of it. `StampLayer` now requires the mode and the armed kind
  to agree.
- Placement disarms and returns to no tool. A rubber stamp repeats by design; a
  signature does not.
- The mode has no palette, so it carries a hint, a Cancel button, and Escape.

```
npm run check                                    # clean
npx vitest src/view stamp-layer + SignatureDialog  # 38 ok (11 + 27)
npm run test                                     # 627 passed (115 files)
```

`npm run test:rust` not re-run — no Rust file touched.

Mutations, both caught:

```
# A. mode no longer has to match the armed kind => 1 failed
# B. stays armed after placing                   => 1 failed
```

Still open, deliberately: the annotation panel lists a placed signature as
**Stamp**, because that is its `/Subtype`. Changing it needs a marker written on
the annotation and read back — see the note in the handover.

### Follow-up 2 — "draw and image won't place, typed works" (not a bug)

It was the decline guard, working. Diagnosed by tracing the click path and
comparing the logged coordinates against the fixture's field rects:

```
click            x       y   outcome    inside which /Sig field?
typed        279.3   616.5   placed     — none —
image        263.5   532.6   declined   Signature1   (170 512 400 545)
image        177.1   621.2   placed     — none —
draw         231.0   448.1   declined   WitnessSig   (190 432 280 452)
draw         230.6   448.1   declined   WitnessSig
draw         208.7   531.2   declined   Signature1
draw         164.7   338.7   placed     — none —
```

Both draw and image place correctly when the click is clear of a field. The
kind correlation was coincidence. Four `Can't place a signature there` warnings
are in the dev log, one per decline — the toasts fired and were not noticed.

**The real defect was the timing of the message**, so the fix is an affordance:
while a signature is armed, every `/Sig` field on the page is outlined in amber
with "Needs a certificate". Advisory only — `pointerEvents: none`, and the click
handler still re-reads the fields and decides for itself, so a stale outline can
never let a signature through onto a signature widget.

```
npx vitest run src/view/__tests__/stamp-layer.test.tsx   # 15 ok (4 new)
npm run check                                            # clean
npm run test                                             # 631 passed (115 files)
```

Mutations, both caught:

```
# A. marker stops filtering by /FT   => 1 failed (marks only signature fields)
# B. marker takes pointer events     => 1 failed (must never eat the click)
```

`npm run test:rust` not re-run — no Rust file touched.

Diagnostics used and removed: temporary `console.warn` tracing in
`stamp-layer.tsx` and `SignatureDialog.tsx`, plus a throwaway
`src-tauri/tests/zz_tmp_probe.rs` that ran every PNG in the real library through
`add_image_stamp` (all five embedded fine, which is what ruled the backend out).
None of it is committed.

### Follow-up 3 — "every signature needs selecting twice"

Regression from the mode split in `dcf4e2e`. `MarkupToolbar` carried
`if (activeTool !== "stamp") armStamp(null)` since P3; adding `"signature"` as a
second tool driving the same layer updated the layer and not the toolbar, so the
toolbar treated signature mode as "left the stamp tool" and cleared the arm the
dialog had just set. The second Place worked because `activeTool` was already
`"signature"` — no dependency change, no effect re-run.

The fact now lives once, in `usesStampLayer` (`tools/stamp/stamps.ts`), used by
both the toolbar's disarm effect and the layer's `active` computation.

```
npx vitest run src/app/__tests__/MarkupToolbar.test.tsx   # 6 ok (new file)
npm run check                                             # clean after one missing import
npm run test                                              # 637 passed (115 files)
```

`npm run test:rust` not re-run — no Rust file touched.

Two things worth remembering:

- The first version of the regression test mutated the zustand stores **outside
  `act()`**, so React never flushed the effect and it passed against the broken
  code. Wrapping each mutation in `act()` fixed it; reinstating the shipped
  condition then failed exactly one test, which is the only proof that matters.
- The trace from follow-up 2 had already printed `after arm, store = null` on
  the first attempt and a populated store on the second. The answer was in a log
  that had been read.

### Follow-up 4 — placing on a /Sig field is now allowed, with a warning

Prompted by Preview offering to sign the same fields we refused. Inspecting the
saved file showed what Preview's feature actually is:

```
/ByteRange      absent   -> no cryptographic signature
/Adobe.PPKLite  absent   -> no signature handler
/FT /Sig x2     present, no /V -> both fields still unsigned
```

So the refusal blocked the most natural action in the document without
preventing the harm it was justified by — that same picture one pixel outside
the box was always allowed, and produces an identical document. Replaced with
a `plugin-dialog` `ask()` modal, once per run:

  "…is a signature field. This places a picture of your signature. It is not a
   digital signature — nothing in the document can be verified, and the field
   itself stays empty."

`declineMessage` -> `pictureWarning`; the amber markers now read
"Picture, not signed".

```
npx vitest run src/tools/signature src/view/__tests__/stamp-layer.test.tsx  # ok
npm run check                                            # clean
npm run test                                             # 642 passed (115 files)
```

Mutations, all three caught by the right test:

```
# A. warning never shown              => 3 failed
# B. declining the warning still places => 1 failed
# C. warns on every click               => 1 failed
```

`npm run test:rust` not re-run — no Rust file touched.

The "asks only once" test needed `act()` around the re-arm between its two
clicks, or React had not re-rendered and the second click hit an inert layer.
Second time in one session.

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
