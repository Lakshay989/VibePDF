// SPEC: P3-ANN-007 (P3.C4a) — the measure tool's mode + per-document calibration,
// plus the calibrate handshake. The toolbar controls write the active kind and
// start calibration; the page MeasureLayer reads them, captures a reference draw,
// and hands its point-length back via `pendingRefPoints` for the dialog to turn
// into a scale. Distinct store so the toolbar and the distant page layer stay in
// sync without prop-drilling.

import { create } from "zustand";

import { type Calibration, DEFAULT_CALIBRATION, type MeasureKind } from "@/tools/measure/measure";

interface MeasureState {
  /** Active measurement type. */
  kind: MeasureKind;
  /** Per-document calibration; missing = uncalibrated (points). */
  calibration: Record<string, Calibration>;
  /** True while the layer is waiting for the user to draw a reference segment. */
  calibrating: boolean;
  /** The drawn reference length (PDF points) awaiting a real-world value, or null. */
  pendingRefPoints: number | null;
  setKind: (k: MeasureKind) => void;
  setCalibration: (docId: string, cal: Calibration) => void;
  startCalibrating: () => void;
  cancelCalibrating: () => void;
  setPendingRef: (points: number | null) => void;
}

export const useMeasureStore = create<MeasureState>((set) => ({
  kind: "distance",
  calibration: {},
  calibrating: false,
  pendingRefPoints: null,
  setKind: (k) => set({ kind: k }),
  setCalibration: (docId, cal) =>
    set((s) => ({
      calibration: { ...s.calibration, [docId]: cal },
      calibrating: false,
      pendingRefPoints: null,
    })),
  startCalibrating: () => set({ calibrating: true, pendingRefPoints: null }),
  cancelCalibrating: () => set({ calibrating: false, pendingRefPoints: null }),
  setPendingRef: (points) => set({ pendingRefPoints: points, calibrating: false }),
}));

/** The active calibration for `docId` (defaults to uncalibrated points). */
export function calibrationFor(
  calibration: Record<string, Calibration>,
  docId: string,
): Calibration {
  return calibration[docId] ?? DEFAULT_CALIBRATION;
}
