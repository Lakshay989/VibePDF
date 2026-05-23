import { describe, it, expect, vi } from "vitest";
import { readFile } from "node:fs/promises";
import path from "node:path";

// PDF.js's main entry expects a real Web Worker. In jsdom we use the
// "legacy" build with `disableWorker: true` semantics by stubbing the
// worker URL to the same thread. This is acceptable for the smoke test
// because we're only checking that the parser can read the byte stream
// and return a non-empty page count.
vi.mock("@/view/pdfjs-worker", () => ({
  configurePdfJsWorker: () => {},
}));

describe("PDF.js render-page smoke", () => {
  it("opens hello.pdf and exposes one page", async () => {
    const { loadDocument } = await import("@/view/render-page");
    const pdfBytes = await readFile(
      path.join(__dirname, "../../../tests/fixtures/basic/hello.pdf"),
    );

    // Force PDF.js into "no worker" mode for jsdom. The library checks
    // GlobalWorkerOptions.workerPort; setting it to null tells it to run
    // the parser on the main thread.
    const pdfjs = await import("pdfjs-dist");
    pdfjs.GlobalWorkerOptions.workerSrc = "";

    const doc = await loadDocument(new Uint8Array(pdfBytes));
    expect(doc.numPages).toBe(1);
    await doc.destroy();
  });
});
