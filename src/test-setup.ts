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

// jsdom (as of v25) does not implement PointerEvent, so testing-library's
// `fireEvent.pointerDown(...)` silently falls back to a bare Event with no
// `clientX`/`clientY` — pointer-driven component tests (the annotation layer,
// future drawing tools) then read `undefined` coordinates. A thin subclass of
// MouseEvent (which jsdom *does* implement, including clientX/Y) is enough.
if (typeof globalThis.PointerEvent === "undefined") {
  interface PointerInit {
    clientX?: number;
    clientY?: number;
    pointerId?: number;
    pressure?: number;
    button?: number;
    buttons?: number;
    bubbles?: boolean;
    cancelable?: boolean;
  }
  const Base = globalThis.MouseEvent;
  class JsdomPointerEvent extends Base {
    public readonly pointerId: number;
    public readonly pressure: number;
    constructor(type: string, params: PointerInit = {}) {
      super(type, params);
      this.pointerId = params.pointerId ?? 0;
      this.pressure = params.pressure ?? 0;
    }
  }
  const ctor = JsdomPointerEvent as unknown;
  (globalThis as unknown as { PointerEvent: unknown }).PointerEvent = ctor;
  if (typeof window !== "undefined") {
    (window as unknown as { PointerEvent: unknown }).PointerEvent = ctor;
  }
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
