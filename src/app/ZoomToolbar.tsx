import { MIN_ZOOM, MAX_ZOOM, useViewStore, type FitMode } from "@/state/view-store";

// SPEC: P1-VIEW-006 (P1.C2).
//
// One toolbar bound to the view-store. Zoom adjustments clear the fit
// mode (the spec implies zoom and fit are mutually exclusive: a fixed
// "175 %" is not also "fit-width"). Fit-mode changes leave zoom
// untouched so toggling back to "Actual" restores the prior numeric.

const ZOOM_STEPS = [0.25, 0.33, 0.5, 0.75, 1, 1.25, 1.5, 1.75, 2, 3, 4, 6, 8, 12, 16];

function nextZoom(current: number, direction: 1 | -1): number {
  if (direction === 1) {
    return ZOOM_STEPS.find((z) => z > current + 1e-6) ?? MAX_ZOOM;
  }
  const lower = [...ZOOM_STEPS].reverse().find((z) => z < current - 1e-6);
  return lower ?? MIN_ZOOM;
}

export function ZoomToolbar() {
  const zoom = useViewStore((s) => s.zoom);
  const fitMode = useViewStore((s) => s.fitMode);
  const setZoom = useViewStore((s) => s.setZoom);
  const setFitMode = useViewStore((s) => s.setFitMode);
  const showOutline = useViewStore((s) => s.showOutline);
  const toggleOutline = useViewStore((s) => s.toggleOutline);
  const showThumbnails = useViewStore((s) => s.showThumbnails);
  const toggleThumbnails = useViewStore((s) => s.toggleThumbnails);

  const fitOptions: { value: FitMode | "manual"; label: string }[] = [
    { value: "manual", label: "Manual" },
    { value: "actual", label: "Actual size" },
    { value: "fit-page", label: "Fit page" },
    { value: "fit-width", label: "Fit width" },
    { value: "fit-height", label: "Fit height" },
  ];

  const selectValue: FitMode | "manual" = fitMode ?? "manual";

  return (
    <div className="flex items-center gap-2 border-b border-neutral-200 px-3 py-1.5 text-sm dark:border-neutral-800">
      <button
        type="button"
        onClick={toggleThumbnails}
        aria-label="Toggle thumbnails sidebar"
        aria-pressed={showThumbnails}
        title="Toggle thumbnails"
        className={
          "rounded px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800 " +
          (showThumbnails ? "bg-neutral-200 dark:bg-neutral-800" : "")
        }
      >
        Pages
      </button>
      <button
        type="button"
        onClick={toggleOutline}
        aria-label="Toggle outline sidebar"
        aria-pressed={showOutline}
        title="Toggle outline"
        className={
          "rounded px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800 " +
          (showOutline ? "bg-neutral-200 dark:bg-neutral-800" : "")
        }
      >
        Outline
      </button>
      <span className="text-neutral-300 dark:text-neutral-700">|</span>
      <button
        type="button"
        onClick={() => setZoom(nextZoom(zoom, -1))}
        aria-label="Zoom out"
        title="Zoom out (⌘-)"
        className="rounded px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800"
      >
        −
      </button>
      <span className="min-w-[4ch] text-center tabular-nums">
        {Math.round(zoom * 100)}%
      </span>
      <button
        type="button"
        onClick={() => setZoom(nextZoom(zoom, +1))}
        aria-label="Zoom in"
        title="Zoom in (⌘=)"
        className="rounded px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800"
      >
        +
      </button>
      <select
        value={selectValue}
        onChange={(e) => {
          const v = e.target.value as FitMode | "manual";
          if (v === "manual") setZoom(zoom);
          else setFitMode(v);
        }}
        className="ml-2 rounded border border-neutral-300 bg-transparent px-1 py-0.5 dark:border-neutral-700"
        aria-label="Fit mode"
      >
        {fitOptions.map((o) => (
          <option key={o.value} value={o.value}>
            {o.label}
          </option>
        ))}
      </select>
    </div>
  );
}
