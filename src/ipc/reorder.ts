import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";
import type { HistoryState } from "@/ipc/history";

/**
 * SPEC: P2-PAGE-002 — reorder the document's pages. `newOrder[newPos] =
 * oldIndex` (0-based), a permutation of `0..pageCount`. Undoable. The reorder
 * runs through the lopdf COS layer on the Rust document actor — the frontend
 * never touches PDF bytes. Object-ref links/bookmarks/destinations track the
 * move automatically.
 */
export async function reorderPages(
  id: DocumentId,
  newOrder: number[],
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_reorder_pages", { id, newOrder });
}
