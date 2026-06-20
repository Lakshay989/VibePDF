import type { HistoryState } from "@/ipc/history";
import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

/**
 * SPEC: P3-ANN-004 — add a line annotation from `(x1,y1)` to `(x2,y2)` (PDF
 * points) on `page` (0-based). `arrow` adds an open-arrow head at the end. The
 * write runs on the Rust document actor (lopdf, with a generated `/AP`) — the
 * frontend never touches PDF bytes. Returns the new undo/redo availability.
 */
export async function addLine(
  id: DocumentId,
  page: number,
  x1: number,
  y1: number,
  x2: number,
  y2: number,
  arrow: boolean,
  strokeColor: string,
  opacity: number,
  strokeWidth: number,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_add_line", {
    id,
    page,
    x1,
    y1,
    x2,
    y2,
    arrow,
    strokeColor,
    opacity,
    strokeWidth,
  });
}
