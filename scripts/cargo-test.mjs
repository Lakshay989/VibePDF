#!/usr/bin/env node
// Cross-platform `cargo test` wrapper. It runs each test binary's tests one at
// a time (see below) and forwards any extra args to cargo, e.g.:
//   node scripts/cargo-test.mjs --test render_compare
//
// Until P7.HF5 this also put src-tauri/resources/pdfium/ on the loader's
// search path (DYLD_LIBRARY_PATH / LD_LIBRARY_PATH / PATH), because the Rust
// side loaded PDFium by bare name. It now loads it by full path
// (src-tauri/src/bundled.rs, SPEC: NFR-PKG-001), so the variable is gone on
// purpose: on CI nothing is on the search path, so a test run fails there if
// any code goes back to searching for the library. A developer machine is not
// a reliable check of that (cargo adds ~/lib and /usr/local/lib to the search
// path on macOS), which is why src-tauri/clippy.toml also forbids the call.

import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const manifest = join(repoRoot, "src-tauri", "Cargo.toml");

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
const result = spawnSync("cargo", args, { stdio: "inherit" });

if (result.error) {
  console.error(`cargo-test: failed to launch cargo: ${result.error.message}`);
  process.exit(1);
}
process.exit(result.status ?? 1);
