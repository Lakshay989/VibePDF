# End-to-end tests (WebdriverIO + tauri-driver)

The only test layer that drives the **real built app** — the Tauri
webview, the Rust backend, PDFium, and PDF.js together. Everything else
(`vitest`, `cargo test`) tests pieces in isolation; this is what would
have caught the "PDF.js worker missing → nothing renders" class of bug.

## Platform: Linux or Windows only

`tauri-driver` wraps the platform WebDriver — `WebKitWebDriver` on Linux,
Edge WebDriver on Windows. **There is no macOS support** (no WKWebView
WebDriver), so `npm run test:e2e` cannot run on a Mac. It runs in CI on
Linux (`.github/workflows/e2e.yml`).

## How the smoke test opens a PDF

The file-open picker is a **native OS dialog**, which a webview
WebDriver can't drive. So the harness launches the app with a PDF path
as a **command-line argument** (the P1.A2 CLI-open path) and asserts the
page renders. `wdio.conf.ts` passes `tests/fixtures/basic/hello.pdf` as
that arg; `specs/smoke.e2e.ts` waits for `[data-page="1"] canvas`.

## Running locally (Linux)

```bash
# 1. System deps (Ubuntu): WebView + WebDriver + virtual display
sudo apt-get install -y libwebkit2gtk-4.1-dev webkit2gtk-driver xvfb

# 2. tauri-driver (Rust binary)
cargo install tauri-driver --locked

# 3. App prerequisites
npm ci                 # postinstall copies the PDF.js worker into public/
npm run fetch-pdfium   # drops libpdfium.so into src-tauri/resources/pdfium/

# 4. Build the app (debug, unbundled)
npx tauri build --debug --no-bundle

# 5. Run (xvfb for a headless display)
xvfb-run -a npm run test:e2e
```

No `LD_LIBRARY_PATH` is needed: the app loads PDFium by full path from its
own resource folder, which for an unbundled build is `src-tauri/target/debug/`
(the build script copies `resources/` there).

CI goes one step further (`.github/workflows/e2e.yml`): it builds a `.deb`,
installs it, runs `vibepdf --self-check`, and points this suite at the
installed binary with `VIBEPDF_BIN`. That is the app as a user gets it
(SPEC: NFR-PKG-001).

Overrides: `VIBEPDF_BIN` (path to the built binary) and
`TAURI_DRIVER_BIN` (path to tauri-driver) if they aren't in the default
locations.

## Adding tests

Drop a `*.e2e.ts` file in `specs/`. Each later UI feature step should add
its own end-to-end test here.
