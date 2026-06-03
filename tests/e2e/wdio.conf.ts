// SPEC: P1.E5 — end-to-end harness (infrastructure).
//
// WebdriverIO config that drives the REAL built VibePDF app via
// `tauri-driver` (W3C WebDriver). This is the only test layer that
// exercises the actual Tauri webview + Rust backend + PDFium + PDF.js
// together — the layer every other test is blind to.
//
// PLATFORM: Linux/Windows ONLY. `tauri-driver` has no macOS support
// (there is no WKWebView WebDriver). On macOS this will fail to start;
// run it on Linux CI (see .github/workflows/e2e.yml).
//
// ENTRY STRATEGY: the file-open picker is a native OS dialog, which a
// webview WebDriver cannot drive. So the smoke test launches the app
// with a PDF path as a CLI argument (exercising A2's CLI-open path) and
// asserts the page renders.

import { spawn, type ChildProcess } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import os from "node:os";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "..", "..");

// The built app binary (debug, unbundled — see e2e.yml). Override with
// VIBEPDF_BIN if you build a release binary or a different target.
const appBinary =
  process.env.VIBEPDF_BIN ??
  resolve(repoRoot, "src-tauri", "target", "debug", "vibepdf");

// Fixture opened on launch via the CLI-arg path.
const helloPdf = resolve(repoRoot, "tests", "fixtures", "basic", "hello.pdf");

// `tauri-driver` is a cargo binary (`cargo install tauri-driver`).
const tauriDriverBin =
  process.env.TAURI_DRIVER_BIN ??
  resolve(os.homedir(), ".cargo", "bin", "tauri-driver");

let tauriDriver: ChildProcess | undefined;

export const config: WebdriverIO.Config = {
  runner: "local",
  hostname: "127.0.0.1",
  port: 4444,
  specs: [resolve(here, "specs", "**", "*.e2e.ts")],
  maxInstances: 1,

  capabilities: [
    {
      // tauri-driver reads these custom capabilities to launch the app.
      // @ts-expect-error — `tauri:options` is a tauri-driver extension,
      // not in the standard WebdriverIO capability types.
      "tauri:options": {
        application: appBinary,
        args: [helloPdf],
      },
    },
  ],

  reporters: ["spec"],
  framework: "mocha",
  mochaOpts: {
    ui: "bdd",
    // Cold app start + first render can be slow on a CI runner.
    timeout: 120_000,
  },
  logLevel: "warn",

  // Spawn tauri-driver as the WebDriver server before the session, and
  // tear it down after.
  beforeSession() {
    tauriDriver = spawn(tauriDriverBin, [], {
      stdio: [null, process.stdout, process.stderr],
    });
    tauriDriver.on("error", (e) => {
      console.error(
        `tauri-driver failed to launch (${tauriDriverBin}): ${e.message}\n` +
          "Install it with `cargo install tauri-driver` and ensure the " +
          "platform WebDriver (Linux: webkit2gtk-driver) is present.",
      );
    });
  },

  afterSession() {
    tauriDriver?.kill();
  },
};
