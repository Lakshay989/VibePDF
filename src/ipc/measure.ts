import type { HistoryState } from "@/ipc/history";
import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";
import type { MeasureKind } from "@/tools/measure/measure";

/** A vertex in PDF points: `[x, y]`. */
export type MeasurePoint = [number, number];

/**
 * SPEC: P3-ANN-007 — add a measurement annotation (`kind` distance|perimeter|
 * area) through `points` (PDF points) on `page` (0-based). `label` is the value
 * already formatted against the user's calibration. The write runs on the Rust
 * document actor (lopdf, with a generated `/AP`) — the frontend never touches
 * PDF bytes. Returns the new undo/redo availability.
 */
export async function addMeasure(
  id: DocumentId,
  page: number,
  kind: MeasureKind,
  points: MeasurePoint[],
  color: string,
  label: string,
  opacity: number,
  strokeWidth: number,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_add_measure", {
    id,
    page,
    kind,
    points,
    color,
    label,
    opacity,
    strokeWidth,
  });
}
