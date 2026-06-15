import { getDocument, type PDFDocumentProxy } from "pdfjs-dist";
import { configurePdfJsWorker } from "@/view/pdfjs-worker";

export interface RenderOutput {
  pageNumber: number;
  width: number;
  height: number;
}

export interface RenderPageOnDocInput {
  doc: PDFDocumentProxy;
  pageNumber: number;
  scale: number;
  canvas: HTMLCanvasElement | OffscreenCanvas;
  /**
   * Device pixel ratio. The canvas *backing store* is rendered at
   * `scale × dpr` physical pixels and displayed at `scale` CSS pixels,
   * so text stays crisp on HiDPI (retina) screens. Defaults to
   * `window.devicePixelRatio`.
   */
  dpr?: number;
  /**
   * Extra clockwise rotation in degrees (a multiple of 90), *added* to the
   * page's own rotation. Used for the rotate fast-path: PDFium holds the
   * real `/Rotate`, and the main view previews it via the viewport rotation
   * without re-parsing the document. Defaults to 0.
   */
  rotation?: number;
}

export async function loadDocument(
  data: Uint8Array,
): Promise<PDFDocumentProxy> {
  configurePdfJsWorker();
  // SPEC: P3-ANN-001 — `getTextContent` (text selection for markup) needs the
  // standard-14 font data to map glyphs → Unicode; rendering falls back to
  // built-in metrics, but text extraction throws without these. cmaps cover
  // CID-keyed fonts. Both are hosted as static assets next to the worker.
  const origin = window.location.origin;
  const task = getDocument({
    data,
    standardFontDataUrl: new URL("/pdfjs/standard_fonts/", origin).toString(),
    cMapUrl: new URL("/pdfjs/cmaps/", origin).toString(),
    cMapPacked: true,
  });
  return task.promise;
}

/**
 * Render a single page onto a caller-owned canvas using an already-
 * loaded `PDFDocumentProxy`. Use this from a virtualizer that keeps
 * the document alive across many page renders.
 */
export async function renderPageOnDoc(
  input: RenderPageOnDocInput,
): Promise<RenderOutput> {
  const dpr =
    input.dpr ??
    (typeof window !== "undefined" ? window.devicePixelRatio || 1 : 1);
  const page = await input.doc.getPage(input.pageNumber);
  // Render the backing store at the physical resolution (scale × dpr)…
  // `rotation` (added to the page's own) is how the rotate fast-path
  // previews a rotation; 90/270 swap the viewport's width and height.
  const viewport = page.getViewport({
    scale: input.scale * dpr,
    rotation: input.rotation ?? 0,
  });
  input.canvas.width = Math.floor(viewport.width);
  input.canvas.height = Math.floor(viewport.height);
  // …but display it at the logical (CSS) size, so the browser
  // downscales the oversized bitmap → crisp text on HiDPI screens.
  // OffscreenCanvas has no `style`; guard for it.
  if ("style" in input.canvas) {
    input.canvas.style.width = `${Math.floor(viewport.width / dpr)}px`;
    input.canvas.style.height = `${Math.floor(viewport.height / dpr)}px`;
  }
  // PDF.js v5 expects `canvas`; the legacy `canvasContext` field is
  // deprecated. We pass the canvas and let PDF.js manage the 2d
  // context lifecycle.
  await page.render({
    canvas: input.canvas as HTMLCanvasElement,
    viewport,
  }).promise;
  return {
    pageNumber: input.pageNumber,
    width: viewport.width / dpr,
    height: viewport.height / dpr,
  };
}

/**
 * Convenience: load + render + destroy. Suitable for one-shot renders
 * (e.g. the bootstrap smoke test). The virtualizer should NOT use this
 * — it would reload the document for every page.
 */
export async function renderPage(input: {
  data: Uint8Array;
  pageNumber: number;
  scale: number;
  canvas: HTMLCanvasElement | OffscreenCanvas;
}): Promise<RenderOutput> {
  const doc = await loadDocument(input.data);
  try {
    return await renderPageOnDoc({
      doc,
      pageNumber: input.pageNumber,
      scale: input.scale,
      canvas: input.canvas,
    });
  } finally {
    await doc.destroy();
  }
}
