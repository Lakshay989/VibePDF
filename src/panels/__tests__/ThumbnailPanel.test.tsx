import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import type { PDFDocumentProxy } from "pdfjs-dist";

// SPEC: P1-VIEW-008 — regression guard for the thumbnail render path.
//
// pdf_render_page returns Vec<u8>, which Tauri hands back over IPC as a
// plain number[] (not a Uint8Array). The original code read
// `.byteLength` on it → undefined → zero-length buffer → throw → a ⚠ on
// every tile. This test feeds the panel a number[] (the real IPC shape)
// and asserts tiles render <img>s, not the failure glyph.

// renderPage returns the realistic IPC shape: bytes as number[].
vi.mock("@/ipc/pdf", () => ({
  renderPage: vi.fn(async () => ({
    width: 96,
    height: 124,
    format: "png" as const,
    bytes: [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a], // PNG magic, number[]
  })),
}));
vi.mock("@/panels/thumbnail-cache", () => ({
  getThumb: vi.fn(async () => null), // force the render path
  putThumb: vi.fn(async () => {}),
}));

import { ThumbnailPanel } from "@/panels/ThumbnailPanel";

// jsdom has no IntersectionObserver; stub one that reports every
// observed tile as immediately visible so the lazy-load path runs.
class ImmediateIO {
  constructor(private cb: IntersectionObserverCallback) {}
  observe(el: Element) {
    this.cb(
      [{ target: el, isIntersecting: true } as IntersectionObserverEntry],
      this as unknown as IntersectionObserver,
    );
  }
  unobserve() {}
  disconnect() {}
  takeRecords(): IntersectionObserverEntry[] {
    return [];
  }
}

beforeEach(() => {
  vi.stubGlobal("IntersectionObserver", ImmediateIO);
  // jsdom doesn't implement createObjectURL/revokeObjectURL.
  vi.stubGlobal("URL", {
    ...URL,
    createObjectURL: vi.fn(() => "blob:fake"),
    revokeObjectURL: vi.fn(),
  });
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function fakeDoc(pages: number): PDFDocumentProxy {
  return {
    numPages: pages,
    getPage: vi.fn(async () => ({
      getViewport: () => ({ width: 612 }),
    })),
  } as unknown as PDFDocumentProxy;
}

describe("ThumbnailPanel render path", () => {
  it("renders an <img> per page from number[] IPC bytes (no ⚠)", async () => {
    render(
      <ThumbnailPanel
        doc={fakeDoc(2)}
        documentId="doc-x"
        onJump={() => {}}
        darkMode={false}
      />,
    );

    // If the byte handling regressed (the original bug), tiles would
    // show ⚠ and no <img> would ever appear → findAllByRole times out.
    const imgs = await screen.findAllByRole("img");
    expect(imgs.length).toBe(2);
    expect(screen.queryByText("⚠")).toBeNull();
    // Light mode: no invert filter on the thumbnails.
    expect((imgs[0] as HTMLImageElement).style.filter).toBe("");
  });

  it("inverts thumbnails in dark mode so the sidebar matches the main view", async () => {
    // SPEC: P1-VIEW-010 — the thumbnail invert must use the same
    // DARK_PAGE_FILTER the page canvases use, or the sidebar stays light
    // while the main view goes dark (the reported mismatch).
    render(
      <ThumbnailPanel
        doc={fakeDoc(1)}
        documentId="doc-d"
        onJump={() => {}}
        darkMode
      />,
    );

    const img = (await screen.findByRole("img")) as HTMLImageElement;
    expect(img.style.filter).toBe("invert(1) hue-rotate(180deg)");
  });
});
