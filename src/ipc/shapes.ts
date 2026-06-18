import type { HistoryState } from "@/ipc/history";
import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

/** The shape kinds C1a supports (drag-to-size, closed). */
export type ShapeKind = "rectangle" | "ellipse";

/** A shape's bounding box in PDF points: `[x0, y0, x1, y1]`, lower-left first. */
export type ShapeRect = [number, number, number, number];

/**
 * SPEC: P3-ANN-004 — add a shape annotation (`/Square` for a rectangle,
 * `/Circle` for an ellipse) bounded by `rect` (PDF points) on `page` (0-based).
 * `fillColor` is `null` for no fill. The write runs on the Rust document actor
 * (lopdf, with a generated `/AP`) — the frontend never touches PDF bytes.
 * Returns the new undo/redo availability.
 */
export async function addShape(
  id: DocumentId,
  page: number,
  kind: ShapeKind,
  rect: ShapeRect,
  strokeColor: string,
  fillColor: string | null,
  opacity: number,
  strokeWidth: number,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_add_shape", {
    id,
    page,
    kind,
    rect,
    strokeColor,
    fillColor,
    opacity,
    strokeWidth,
  });
}
