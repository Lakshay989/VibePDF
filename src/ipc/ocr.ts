import type { HistoryState } from "@/ipc/history";
import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

/**
 * What one OCR run found. Mirrors `pdf::ocr_text_layer::OcrSummary`.
 *
 * `skipped` counts words Tesseract read but that were not written: below the
 * confidence floor, or in characters the bundled font can't encode. A page can
 * legitimately produce `words: 0` — a photograph has nothing to read.
 */
export interface OcrSummary {
  pages: number;
  words: number;
  skipped: number;
  milliseconds: number;
}

export interface OcrRunReply {
  summary: OcrSummary;
  history: HistoryState;
}

/** The configurable pipeline of P7-OCR-003. Every field falls back to Rust's default. */
export interface OcrOptions {
  /** Tesseract language code; only `"eng"` ships today (P7.A3 adds the rest). */
  language?: string;
  /** Render resolution for recognition. Default 300. */
  dpi?: number;
  /** Drop words the engine is less sure of than this, 0–1. Default 0.5. */
  minConfidence?: number;
  deskew?: boolean;
  denoise?: boolean;
  /** Upscale anything rendered below this. 0 disables upscaling. */
  minDpi?: number;
}

/**
 * SPEC: P7-OCR-001 — OCR the given 0-based pages (all of them when `pages` is
 * empty) and add an invisible text layer, making the document searchable.
 *
 * One undoable edit for the whole run. It is slow by nature — on the order of a
 * second per page — and the document's actor handles one job at a time, so the
 * caller should expect this to block other edits to that document until it
 * returns. There is no progress reporting yet.
 */
export async function runOcr(
  id: DocumentId,
  pages: number[] = [],
  options: OcrOptions = {},
): Promise<OcrRunReply> {
  return invoke<OcrRunReply>("pdf_ocr_run", {
    id,
    pages,
    language: options.language ?? null,
    dpi: options.dpi ?? null,
    minConfidence: options.minConfidence ?? null,
    deskew: options.deskew ?? null,
    denoise: options.denoise ?? null,
    minDpi: options.minDpi ?? null,
  });
}
