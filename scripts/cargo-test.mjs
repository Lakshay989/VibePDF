#!/usr/bin/env node
// Cross-platform `cargo test` wrapper that puts the fetched PDFium
// dynamic library on the loader's search path before running the Rust
// tests. Without this the PDF-touching tests fail with a
// `LoadLibraryError` — the lib lives in src-tauri/resources/pdfium/
// (gitignored, fetched by scripts/fetch-pdfium.sh) and is not on any
// default search path at `cargo test` time.
//
// The env var differs per OS:
//   macOS   → DYLD_LIBRARY_PATH
//   Linux   → LD_LIBRARY_PATH
//   Windows → PATH (PDFium ships as a .dll)
//
// Any extra args are forwarded to cargo, e.g.:
//   node scripts/cargo-test.mjs --test render_compare

import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const pdfiumDir = join(repoRoot, "src-tauri", "resources", "pdfium");
const manifest = join(repoRoot, "src-tauri", "Cargo.toml");

const env = { ...process.env };
const sep = process.platform === "win32" ? ";" : ":";
const varName =
  process.platform === "darwin"
    ? "DYLD_LIBRARY_PATH"
    : process.platform === "win32"
      ? "PATH"
      : "LD_LIBRARY_PATH";
env[varName] = env[varName] ? `${pdfiumDir}${sep}${env[varName]}` : pdfiumDir;

// PDFium is not thread-safe across documents — even FPDF_CloseDocument
// (PdfDocument's Drop) races other threads' PDFium calls. The library
// code serializes operations through a process-global lock, but the
// integration tests open and drop their *own* documents, which can't take
// that pub(crate) lock. So run the test harness single-threaded; without
// this, the PDF-touching tests SIGSEGV/SIGABRT intermittently under
// cargo's default parallel runner. (Test binaries are separate processes,
// each with its own PDFium, so cross-binary parallelism stays safe.)
const forwarded = process.argv.slice(2);
const hasThreadFlag = forwarded.some((a) => a.startsWith("--test-threads"));
let harnessForwarded;
if (forwarded.includes("--")) {
  const i = forwarded.indexOf("--");
  harnessForwarded = hasThreadFlag
    ? forwarded
    : [...forwarded.slice(0, i + 1), "--test-threads=1", ...forwarded.slice(i + 1)];
} else {
  harnessForwarded = hasThreadFlag
    ? forwarded
    : [...forwarded, "--", "--test-threads=1"];
}

const args = ["test", "--manifest-path", manifest, ...harnessForwarded];
const result = spawnSync("cargo", args, { stdio: "inherit", env });

if (result.error) {
  console.error(`cargo-test: failed to launch cargo: ${result.error.message}`);
  process.exit(1);
}
process.exit(result.status ?? 1);
