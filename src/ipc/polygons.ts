import type { HistoryState } from "@/ipc/history";
import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

/** A vertex in PDF points: `[x, y]`. */
export type PolygonPoint = [number, number];

/**
 * SPEC: P3-ANN-004 — add a polygon (`closed`, fillable) or polyline (`!closed`)
 * through `points` (PDF points) on `page` (0-based). The write runs on the Rust
 * document actor (lopdf, with a generated `/AP`) — the frontend never touches PDF
 * bytes. Returns the new undo/redo availability.
 */
export async function addPolygon(
  id: DocumentId,
  page: number,
  closed: boolean,
  points: PolygonPoint[],
  strokeColor: string,
  fillColor: string | null,
  opacity: number,
  strokeWidth: number,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_add_polygon", {
    id,
    page,
    closed,
    points,
    strokeColor,
    fillColor,
    opacity,
    strokeWidth,
  });
}
