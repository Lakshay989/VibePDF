// SPEC: P3-ANN-007 (P3.C4a) — the measure controls shown in the toolbar while
// the Measure tool is active: the mode (distance / perimeter / area), a Calibrate
// action, and the current scale readout. Hosts the calibrate dialog.

import { calibrationFor, useMeasureStore } from "@/state/measure-store";
import { CalibrateDialog } from "@/tools/measure/CalibrateDialog";
import { DEFAULT_CALIBRATION, type MeasureKind } from "@/tools/measure/measure";
import { useCalibrationSync } from "@/tools/measure/use-calibration-sync";

const KINDS: { id: MeasureKind; label: string }[] = [
  { id: "distance", label: "Distance" },
  { id: "perimeter", label: "Perimeter" },
  { id: "area", label: "Area" },
];

export function MeasureControls({ documentId }: { documentId: string }) {
  // SPEC: P3-ANN-007 (P3.C4b) — restore a saved calibration when the tool opens.
  useCalibrationSync(documentId);
  const kind = useMeasureStore((s) => s.kind);
  const setKind = useMeasureStore((s) => s.setKind);
  const calibrating = useMeasureStore((s) => s.calibrating);
  const startCalibrating = useMeasureStore((s) => s.startCalibrating);
  const calibration = useMeasureStore((s) => s.calibration);
  const cal = calibrationFor(calibration, documentId);
  const uncalibrated = cal.unitsPerPoint === DEFAULT_CALIBRATION.unitsPerPoint && cal.unit === DEFAULT_CALIBRATION.unit;

  return (
    <div className="flex items-center gap-1">
      {KINDS.map((k) => (
        <button
          key={k.id}
          type="button"
          onClick={() => setKind(k.id)}
          aria-pressed={kind === k.id}
          className={
            "rounded px-2 py-0.5 text-xs " +
            (kind === k.id
              ? "bg-blue-200 dark:bg-blue-300/30"
              : "hover:bg-neutral-100 dark:hover:bg-neutral-800")
          }
        >
          {k.label}
        </button>
      ))}
      <button
        type="button"
        onClick={startCalibrating}
        aria-pressed={calibrating}
        title="Draw a known length, then enter its real size"
        className={
          "rounded border px-2 py-0.5 text-xs " +
          (calibrating
            ? "border-blue-500 bg-blue-100 text-blue-800 dark:bg-blue-900/40 dark:text-blue-200"
            : "border-neutral-300 hover:bg-neutral-100 dark:border-neutral-600 dark:hover:bg-neutral-800")
        }
      >
        {calibrating ? "Draw reference…" : "Calibrate"}
      </button>
      <span className="text-[10px] tabular-nums text-neutral-400">
        {uncalibrated ? "uncalibrated (pt)" : `1 pt = ${cal.unitsPerPoint.toPrecision(3)} ${cal.unit}`}
      </span>
      <CalibrateDialog documentId={documentId} />
    </div>
  );
}
