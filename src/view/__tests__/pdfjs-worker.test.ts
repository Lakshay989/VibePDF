import { existsSync, readdirSync } from "node:fs";
import { resolve } from "node:path";

import { describe, expect, it } from "vitest";

// SPEC: P1-VIEW-001 — regression guard for the PDF.js worker asset.
//
// `src/view/pdfjs-worker.ts` sets GlobalWorkerOptions.workerSrc to
// `/pdfjs/pdf.worker.min.mjs`, which must resolve to a real file under
// `public/`. If it's missing, PDF.js fails at runtime with
// "Setting up fake worker failed: Importing a module script failed" and
// *every* render reports "not a valid PDF" — but no other test catches
// it, because the render smoke test mocks the worker.
//
// The file is copied from node_modules by scripts/copy-pdfjs-worker.mjs,
// which runs via the `pretest` hook (and postinstall / predev /
// prebuild), so it is present by the time this test runs.
describe("pdf.js worker asset", () => {
  it("is present at the public path workerSrc points to", () => {
    const workerPath = resolve(
      __dirname,
      "../../../public/pdfjs/pdf.worker.min.mjs",
    );
    expect(
      existsSync(workerPath),
      "public/pdfjs/pdf.worker.min.mjs missing — run `node scripts/copy-pdfjs-worker.mjs` " +
        "(normally automatic via the postinstall/predev/prebuild/pretest hooks)",
    ).toBe(true);
  });

  // SPEC: P1-VIEW-004 — the image decoders `wasmUrl` points at. Decoding is
  // proved in image-decoders.test.ts; this names what must be served.
  it("serves the image decoders and the CMYK profile", () => {
    const pdfjs = resolve(__dirname, "../../../public/pdfjs");
    for (const file of [
      "wasm/jbig2.wasm",
      "wasm/jbig2_nowasm_fallback.js",
      "wasm/openjpeg.wasm",
      "wasm/openjpeg_nowasm_fallback.js",
      "wasm/qcms_bg.wasm",
      "iccs/CGATS001Compat-v2-micro.icc",
    ]) {
      expect(existsSync(resolve(pdfjs, file)), `public/pdfjs/${file} missing`).toBe(true);
    }
  });

  // PDF.js's sandbox for running a document's own JavaScript ships in the same
  // directory. VibePDF never runs document scripts (pdfjs-surface.test.ts), and
  // not serving the sandbox keeps any future option from switching it on.
  it("does not serve PDF.js's document-script sandbox", () => {
    const wasm = resolve(__dirname, "../../../public/pdfjs/wasm");
    expect(readdirSync(wasm).filter((f) => f.startsWith("quickjs"))).toEqual([]);
  });
});
