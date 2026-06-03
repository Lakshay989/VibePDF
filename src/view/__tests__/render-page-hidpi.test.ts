import { describe, expect, it } from "vitest";
import type { PDFDocumentProxy } from "pdfjs-dist";

import { renderPageOnDoc } from "@/view/render-page";

// SPEC: P1-VIEW-001 / NFR — HiDPI regression guard.
//
// renderPageOnDoc must render the canvas *backing store* at
// `scale × dpr` physical pixels and display it at `scale` CSS pixels.
// Sizing the backing store at logical pixels (the original bug) makes
// text blurry on retina screens. A fake doc/page lets us assert the
// sizing without a real PDF or canvas API.
function fakeDoc(naturalWidth: number, naturalHeight: number): PDFDocumentProxy {
  const page = {
    getViewport: ({ scale }: { scale: number }) => ({
      width: naturalWidth * scale,
      height: naturalHeight * scale,
    }),
    render: () => ({ promise: Promise.resolve() }),
  };
  return { getPage: async () => page } as unknown as PDFDocumentProxy;
}

describe("renderPageOnDoc HiDPI sizing", () => {
  it("sizes the backing store at scale × dpr and the CSS box at logical size", async () => {
    const canvas = document.createElement("canvas");
    await renderPageOnDoc({
      doc: fakeDoc(200, 300),
      pageNumber: 1,
      scale: 2,
      canvas,
      dpr: 2,
    });

    // backing store = 200 × scale(2) × dpr(2) = 800 (× 300·2·2 = 1200)
    expect(canvas.width).toBe(800);
    expect(canvas.height).toBe(1200);
    // displayed (CSS) at the logical size = backing / dpr
    expect(canvas.style.width).toBe("400px");
    expect(canvas.style.height).toBe("600px");
  });

  it("at dpr = 1 the backing store equals the logical size", async () => {
    const canvas = document.createElement("canvas");
    await renderPageOnDoc({
      doc: fakeDoc(100, 100),
      pageNumber: 1,
      scale: 1,
      canvas,
      dpr: 1,
    });
    expect(canvas.width).toBe(100);
    expect(canvas.style.width).toBe("100px");
  });
});
