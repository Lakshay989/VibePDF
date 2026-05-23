import { getDocument, type PDFDocumentProxy } from "pdfjs-dist";
import { configurePdfJsWorker } from "@/view/pdfjs-worker";

export interface RenderInput {
  data: Uint8Array;
  pageNumber: number;
  scale: number;
  canvas: HTMLCanvasElement | OffscreenCanvas;
}

export interface RenderOutput {
  pageNumber: number;
  width: number;
  height: number;
}

export async function loadDocument(
  data: Uint8Array,
): Promise<PDFDocumentProxy> {
  configurePdfJsWorker();
  const task = getDocument({ data });
  return task.promise;
}

export async function renderPage(input: RenderInput): Promise<RenderOutput> {
  const doc = await loadDocument(input.data);
  try {
    const page = await doc.getPage(input.pageNumber);
    const viewport = page.getViewport({ scale: input.scale });
    input.canvas.width = Math.floor(viewport.width);
    input.canvas.height = Math.floor(viewport.height);
    // PDF.js v5 expects `canvas`; the legacy `canvasContext` field is
    // deprecated. We pass the canvas itself and let PDF.js manage the
    // 2d context lifecycle.
    await page.render({
      canvas: input.canvas as HTMLCanvasElement,
      viewport,
    }).promise;
    return {
      pageNumber: input.pageNumber,
      width: viewport.width,
      height: viewport.height,
    };
  } finally {
    await doc.destroy();
  }
}
