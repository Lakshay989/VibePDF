import type { HistoryState } from "@/ipc/history";
import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

/** A captured ink sample in PDF points: `[x, y, pressure]` (pressure in `[0,1]`). */
export type InkPoint = [number, number, number];

/**
 * SPEC: P3-ANN-005 — add a freehand `/Ink` annotation through `points` (PDF
 * points, smoothing already applied frontend-side) on `page` (0-based). The
 * write runs on the Rust document actor (lopdf, with a generated variable-width
 * `/AP`) — the frontend never touches PDF bytes. Returns the new undo/redo
 * availability.
 */
export async function addInk(
  id: DocumentId,
  page: number,
  points: InkPoint[],
  color: string,
  opacity: number,
  baseWidth: number,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_add_ink", {
    id,
    page,
    points,
    color,
    opacity,
    baseWidth,
  });
}
