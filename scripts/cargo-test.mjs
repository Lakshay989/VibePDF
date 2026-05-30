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

const args = ["test", "--manifest-path", manifest, ...process.argv.slice(2)];
const result = spawnSync("cargo", args, { stdio: "inherit", env });

if (result.error) {
  console.error(`cargo-test: failed to launch cargo: ${result.error.message}`);
  process.exit(1);
}
process.exit(result.status ?? 1);
