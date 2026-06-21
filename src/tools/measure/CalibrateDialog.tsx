// SPEC: P3-ANN-007 (P3.C4a) — after the user draws a reference segment, ask what
// real-world length it represents and store the resulting scale (units/point).

import { useState } from "react";

import { useMeasureStore } from "@/state/measure-store";
import { calibrationScale } from "@/tools/measure/measure";

export function CalibrateDialog({ documentId }: { documentId: string }) {
  const refPoints = useMeasureStore((s) => s.pendingRefPoints);
  const setCalibration = useMeasureStore((s) => s.setCalibration);
  const cancel = useMeasureStore((s) => s.cancelCalibrating);
  const [length, setLength] = useState("1");
  const [unit, setUnit] = useState("m");

  if (refPoints === null) return null;

  const apply = () => {
    const real = Number(length);
    if (!Number.isFinite(real) || real <= 0) return;
    setCalibration(documentId, {
      unitsPerPoint: calibrationScale(refPoints, real),
      unit: unit.trim() || "u",
    });
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/30"
      role="dialog"
      aria-modal="true"
      aria-label="Calibrate measurement scale"
    >
      <div className="w-80 rounded-lg bg-white p-4 shadow-xl dark:bg-neutral-900">
        <h2 className="mb-2 text-sm font-semibold">Calibrate scale</h2>
        <p className="mb-3 text-xs text-neutral-500">
          The reference you drew is {refPoints.toFixed(1)} pt. Enter the real-world length it represents.
        </p>
        <div className="mb-3 flex items-center gap-2">
          <input
            type="number"
            min="0"
            step="any"
            value={length}
            onChange={(e) => setLength(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") apply();
            }}
            aria-label="Real-world length"
            autoFocus
            className="w-24 rounded border border-neutral-300 bg-transparent px-2 py-1 text-sm dark:border-neutral-600"
          />
          <input
            type="text"
            value={unit}
            onChange={(e) => setUnit(e.target.value)}
            aria-label="Unit"
            placeholder="m"
            className="w-20 rounded border border-neutral-300 bg-transparent px-2 py-1 text-sm dark:border-neutral-600"
          />
        </div>
        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={cancel}
            className="rounded px-3 py-1 text-sm hover:bg-neutral-100 dark:hover:bg-neutral-800"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={apply}
            className="rounded bg-blue-600 px-3 py-1 text-sm text-white hover:bg-blue-700"
          >
            Set scale
          </button>
        </div>
      </div>
    </div>
  );
}
