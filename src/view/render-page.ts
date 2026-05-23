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
}

export async function loadDocument(
  data: Uint8Array,
): Promise<PDFDocumentProxy> {
  configurePdfJsWorker();
  const task = getDocument({ data });
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
  const page = await input.doc.getPage(input.pageNumber);
  const viewport = page.getViewport({ scale: input.scale });
  input.canvas.width = Math.floor(viewport.width);
  input.canvas.height = Math.floor(viewport.height);
  // PDF.js v5 expects `canvas`; the legacy `canvasContext` field is
  // deprecated. We pass the canvas and let PDF.js manage the 2d
  // context lifecycle.
  await page.render({
    canvas: input.canvas as HTMLCanvasElement,
    viewport,
  }).promise;
  return {
    pageNumber: input.pageNumber,
    width: viewport.width,
    height: viewport.height,
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
