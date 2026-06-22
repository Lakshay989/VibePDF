import type { HistoryState } from "@/ipc/history";
import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";
import type { Calibration, MeasureKind } from "@/tools/measure/measure";

/** A vertex in PDF points: `[x, y]`. */
export type MeasurePoint = [number, number];

/**
 * SPEC: P3-ANN-007 — add a measurement annotation (`kind` distance|perimeter|
 * area) through `points` (PDF points) on `page` (0-based). `label` is the value
 * already formatted against the user's calibration; `unitsPerPoint` + `unit`
 * carry that calibration so the backend can attach a machine-readable `/Measure`
 * dict (P3.C4b) for live re-measure in other readers. The write runs on the Rust
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
  unitsPerPoint: number,
  unit: string,
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
    unitsPerPoint,
    unit,
  });
}

/**
 * SPEC: P3-ANN-007 (P3.C4b) — read the document's measurement calibration back
 * out of the first `/Measure` dict, so the tool can re-seed itself on reopen
 * instead of forcing a re-calibrate. `null` when no measurement carries one.
 */
export async function readMeasureCalibration(id: DocumentId): Promise<Calibration | null> {
  return invoke<Calibration | null>("pdf_read_measure_calibration", { id });
}
