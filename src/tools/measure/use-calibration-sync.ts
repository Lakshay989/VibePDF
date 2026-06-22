// SPEC: P3-ANN-007 (P3.C4b) — re-seed the measure tool's calibration from the
// document on open, so a saved-then-reopened PDF doesn't force a re-calibrate.
// Reads the calibration back out of the first measurement's `/Measure` dict (the
// inverse of what `add_measure` writes) and seeds the store — but never clobbers
// a calibration the user set in this session.

import { useEffect } from "react";

import { readMeasureCalibration } from "@/ipc/measure";
import { calibrationFor, useMeasureStore } from "@/state/measure-store";
import { DEFAULT_CALIBRATION } from "@/tools/measure/measure";

export function useCalibrationSync(documentId: string): void {
  const setCalibration = useMeasureStore((s) => s.setCalibration);
  useEffect(() => {
    let cancelled = false;
    readMeasureCalibration(documentId)
      .then((cal) => {
        if (cancelled || !cal) return;
        // Only seed when the user hasn't already calibrated this doc this session
        // (read the live store imperatively so this effect doesn't re-run on every
        // calibration change).
        const current = calibrationFor(useMeasureStore.getState().calibration, documentId);
        const isDefault =
          current.unitsPerPoint === DEFAULT_CALIBRATION.unitsPerPoint &&
          current.unit === DEFAULT_CALIBRATION.unit;
        if (isDefault) setCalibration(documentId, cal);
      })
      .catch((err: unknown) => console.warn("read measure calibration failed", documentId, err));
    return () => {
      cancelled = true;
    };
  }, [documentId, setCalibration]);
}
