// SPEC: P3-ANN-001 — WKWebView (the macOS Tauri webview / Safari) does NOT
// implement async iteration on `ReadableStream` (`Symbol.asyncIterator`), but
// PDF.js v5's `getTextContent` does `for await (… of streamTextContent())`. So
// text extraction — and therefore text selection / the text layer / markup —
// throws "undefined is not a function (near '…value of readableStream…')".
//
// Polyfill the async iterator over a reader. Must load before any PDF.js use,
// so it's the first import in `main.tsx`. (See docs/04 "WebView quirks".)

interface StreamReader {
  read: () => Promise<{ done: boolean; value: unknown }>;
  releaseLock: () => void;
}

const rs = (
  globalThis as unknown as { ReadableStream?: { prototype: Record<symbol, unknown> } }
).ReadableStream;

if (rs && typeof rs.prototype[Symbol.asyncIterator] !== "function") {
  rs.prototype[Symbol.asyncIterator] = function asyncIterator(this: {
    getReader: () => StreamReader;
  }) {
    const reader = this.getReader();
    return {
      next: () => reader.read(),
      return() {
        reader.releaseLock();
        return Promise.resolve({ done: true, value: undefined });
      },
      [Symbol.asyncIterator]() {
        return this;
      },
    };
  };
}

export {};
