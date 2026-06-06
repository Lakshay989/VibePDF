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
