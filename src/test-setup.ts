// vitest setup. Polyfills jsdom for the parts of the PDF.js render path
// it needs (matchMedia, getComputedStyle quirks). Add more as smoke
// tests grow.
if (typeof window !== "undefined" && !window.matchMedia) {
  Object.defineProperty(window, "matchMedia", {
    writable: true,
    value: (query: string) => ({
      matches: false,
      media: query,
      onchange: null,
      addEventListener: () => {},
      removeEventListener: () => {},
      addListener: () => {},
      removeListener: () => {},
      dispatchEvent: () => false,
    }),
  });
}

// jsdom (as of v25) does not implement DOMMatrix. PDF.js still touches
// it at module load (`const SCALE_MATRIX = new DOMMatrix();` in
// canvas.js) even in the legacy build, so importing the library would
// throw under vitest without a stub. Our smoke test never rasterises a
// page, so a constructor that swallows the constructor args is enough —
// if a future test actually renders, swap this for the `dommatrix`
// polyfill.
if (typeof globalThis.DOMMatrix === "undefined") {
  class DOMMatrixStub {
    constructor(_init?: string | number[]) {}
  }
  (globalThis as unknown as { DOMMatrix: unknown }).DOMMatrix = DOMMatrixStub;
}

// We also alias `pdfjs-dist` → `pdfjs-dist/legacy/build/pdf.mjs` in the
// vitest config (see vite.config.ts). The legacy bundle self-polyfills
// `Promise.try`, `Uint8Array.prototype.toHex`, etc. via core-js, which
// is what PDF.js's own warning is asking you to do when running in
// Node.
//
// jsdom has no `Worker`, so PDF.js falls back to a "fake worker" that
// would otherwise dynamically import `GlobalWorkerOptions.workerSrc`.
// We don't ship the worker as a static URL in the test env, but PDF.js
// also honours `globalThis.pdfjsWorker.WorkerMessageHandler` as a
// short-circuit — preloading the legacy worker module skips the URL
// lookup entirely, regardless of what individual tests do to workerSrc.
if (typeof (globalThis as { pdfjsWorker?: unknown }).pdfjsWorker === "undefined") {
  // @ts-expect-error -- pdfjs-dist ships no `.d.ts` for the legacy
  // worker subpath, but the file exists in the published tarball.
  const worker = await import("pdfjs-dist/legacy/build/pdf.worker.mjs");
  (globalThis as unknown as { pdfjsWorker: unknown }).pdfjsWorker = worker;
}

// Marker so TS treats this file as a module (top-level `await` requires
// it). Adds no runtime behaviour.
export {};
