import { describe, expect, it } from "vitest";
import type { PDFDocumentProxy } from "pdfjs-dist";

import { renderPageOnDoc } from "@/view/render-page";

// SPEC: P2-PAGE-001 (rotate fast-path) — renderPageOnDoc must forward
// `rotation` to PDF.js's getViewport so the preview reflects a rotation
// without re-parsing. A fake page that swaps width/height for 90°/270°
// (matching PDF.js) lets us assert the pass-through without a real canvas.
function fakeRotatableDoc(w: number, h: number): PDFDocumentProxy {
  const page = {
    getViewport: ({
      scale,
      rotation = 0,
    }: {
      scale: number;
      rotation?: number;
    }) => {
      const swap = rotation % 180 === 90;
      return { width: (swap ? h : w) * scale, height: (swap ? w : h) * scale };
    },
    render: () => ({ promise: Promise.resolve() }),
  };
  return { getPage: async () => page } as unknown as PDFDocumentProxy;
}

describe("renderPageOnDoc rotation", () => {
  it("defaults to no rotation (dimensions unswapped)", async () => {
    const canvas = document.createElement("canvas");
    await renderPageOnDoc({
      doc: fakeRotatableDoc(200, 300),
      pageNumber: 1,
      scale: 1,
      canvas,
      dpr: 1,
    });
    expect(canvas.width).toBe(200);
    expect(canvas.height).toBe(300);
  });

  it("forwards 90° to the viewport (swaps width/height)", async () => {
    const canvas = document.createElement("canvas");
    await renderPageOnDoc({
      doc: fakeRotatableDoc(200, 300),
      pageNumber: 1,
      scale: 1,
      canvas,
      dpr: 1,
      rotation: 90,
    });
    expect(canvas.width).toBe(300);
    expect(canvas.height).toBe(200);
  });

  it("leaves 180° dimensions unswapped (but still rotates)", async () => {
    const canvas = document.createElement("canvas");
    await renderPageOnDoc({
      doc: fakeRotatableDoc(200, 300),
      pageNumber: 1,
      scale: 1,
      canvas,
      dpr: 1,
      rotation: 180,
    });
    expect(canvas.width).toBe(200);
    expect(canvas.height).toBe(300);
  });
});
