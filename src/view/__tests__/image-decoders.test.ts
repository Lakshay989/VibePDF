import { readFile } from "node:fs/promises";
import path from "node:path";

import { describe, expect, it, vi } from "vitest";

// SPEC: P1-VIEW-004 — images compressed as JBIG2, CCITT fax or JPEG 2000 must
// render in the view. PDF.js decodes all three only through WebAssembly
// modules (or their JS fallbacks) fetched from `wasmUrl`. Until 2026-09-15
// VibePDF never set it, and those images silently drew nothing: no error in
// the UI, just a blank where a scanned page should be.
//
// These tests load the fixtures through `loadDocument` with the *served*
// directory (`public/`, filled by scripts/copy-pdfjs-worker.mjs) as the asset
// base, then compare the decoded pixels with the generators' patterns — so the
// copied files, the option names and the paths are covered together.
//
// A negative control can't live in this file: PDF.js caches each decoder
// module for the whole process once it has loaded, so a later load without
// `wasmUrl` still decodes. The regression checks were run as separate runs
// instead (see the commit that added this file).

vi.mock("@/view/pdfjs-worker", () => ({ configurePdfJsWorker: () => {} }));

const repo = path.join(__dirname, "../../..");
const publicDir = `${path.join(repo, "public")}/`;

interface DecodedImage {
  width: number;
  height: number;
  data: Uint8Array | Uint8ClampedArray;
}

/** The one image painted on page 1 of a fixture, as PDF.js decoded it. */
async function decodedImage(fixture: string): Promise<DecodedImage> {
  const pdfjs = await import("pdfjs-dist");
  pdfjs.GlobalWorkerOptions.workerSrc = "";
  const { loadDocument } = await import("@/view/render-page");
  const bytes = await readFile(path.join(repo, "tests/fixtures/basic", fixture));
  const doc = await loadDocument(new Uint8Array(bytes), publicDir);
  try {
    const page = await doc.getPage(1);
    const ops = await page.getOperatorList();
    const at = ops.fnArray.indexOf(pdfjs.OPS.paintImageXObject);
    expect(at, "page 1 paints no image").toBeGreaterThanOrEqual(0);
    const objId = (ops.argsArray[at] as [string])[0];
    return await new Promise<DecodedImage>((resolve) => {
      page.objs.get(objId, (img: DecodedImage) => resolve(img));
    });
  } finally {
    await doc.loadingTask.destroy();
  }
}

describe("PDF.js image decoders", () => {
  it("decode a CCITT fax image (the JBIG2 module) to the generated pattern", async () => {
    // Mirrors BLACK in tests/fixtures/basic/generate-ccitt.py.
    const black = [
      [2, 5, 1, 10],
      [9, 13, 8, 14],
    ];
    const isBlack = (row: number, col: number) =>
      black.some(([r0, r1, c0, c1]) => row >= r0 && row <= r1 && col >= c0 && col <= c1);

    const img = await decodedImage("ccitt.pdf");
    expect([img.width, img.height]).toEqual([16, 16]);
    // One bit per pixel, two bytes per row, 1 = white.
    const mismatched: string[] = [];
    for (let row = 0; row < 16; row++) {
      for (let col = 0; col < 16; col++) {
        const white = (img.data[row * 2 + (col >> 3)] >> (7 - (col % 8))) & 1;
        if ((white === 0) !== isBlack(row, col)) mismatched.push(`${row},${col}`);
      }
    }
    expect(mismatched).toEqual([]);
  });

  it("decode a JPEG 2000 image to the generated pattern", async () => {
    // Mirrors pixel() in tests/fixtures/basic/generate-jpx.py (lossless).
    const expected = (row: number, col: number) =>
      row < 8 ? (col < 8 ? [255, 0, 0] : [0, 255, 0]) : col < 8 ? [0, 0, 255] : [255, 255, 255];

    const img = await decodedImage("jpx.pdf");
    expect([img.width, img.height]).toEqual([16, 16]);
    const mismatched: string[] = [];
    for (let row = 0; row < 16; row++) {
      for (let col = 0; col < 16; col++) {
        const i = (row * 16 + col) * 3;
        if (Array.from(img.data.slice(i, i + 3)).join() !== expected(row, col).join()) {
          mismatched.push(`${row},${col}`);
        }
      }
    }
    expect(mismatched).toEqual([]);
  });
});

describe("pdfjsAssetUrls", () => {
  it("points every PDF.js data directory under /pdfjs/ at the given base", async () => {
    const { pdfjsAssetUrls } = await import("@/view/render-page");
    expect(pdfjsAssetUrls("tauri://localhost/")).toEqual({
      standardFontDataUrl: "tauri://localhost/pdfjs/standard_fonts/",
      cMapUrl: "tauri://localhost/pdfjs/cmaps/",
      wasmUrl: "tauri://localhost/pdfjs/wasm/",
      iccUrl: "tauri://localhost/pdfjs/iccs/",
    });
  });
});
