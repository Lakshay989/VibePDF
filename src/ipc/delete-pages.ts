import type { HistoryState } from "@/ipc/history";
import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

/**
 * SPEC: P2-PAGE-003 — delete `pages` (0-based indices). PDFium renumbers
 * the page tree; the removed pages are preserved for undo. Returns the new
 * undo/redo availability so the caller can refresh the button state.
 */
export async function deletePages(
  id: DocumentId,
  pages: number[],
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_delete_pages", { id, pages });
}
