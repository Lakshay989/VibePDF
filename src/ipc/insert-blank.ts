import type { HistoryState } from "@/ipc/history";
import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

/**
 * SPEC: P2-PAGE-004 — insert a blank page at `index` (0-based; `index ===
 * pageCount` appends). Pass `size` (in points) to set the dimensions;
 * otherwise the new page inherits the adjacent page's size and orientation.
 * Returns the new undo/redo availability.
 */
export async function insertBlankPage(
  id: DocumentId,
  index: number,
  size?: { width: number; height: number },
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_insert_blank_page", {
    id,
    index,
    width: size?.width ?? null,
    height: size?.height ?? null,
  });
}
