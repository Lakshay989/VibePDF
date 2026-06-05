import type { HistoryState } from "@/ipc/history";
import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

/**
 * SPEC: P2-PAGE-001 — rotate `pages` (0-based indices) by `degrees` (a
 * multiple of 90°; positive = clockwise). Persisted as PDFium `/Rotate`,
 * recorded on the undo stack. Returns the new undo/redo availability so
 * the caller can refresh the Undo/Redo button state.
 */
export async function rotatePages(
  id: DocumentId,
  pages: number[],
  degrees: number,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_rotate_pages", { id, pages, degrees });
}
