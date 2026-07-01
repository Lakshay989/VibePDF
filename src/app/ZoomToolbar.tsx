import type { Theme } from "@/app/theme";
import { useSettingsStore } from "@/state/settings-store";
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

export interface ZoomToolbarProps {
  /** SPEC: P2-PAGE-006 — open the extract-pages dialog (when a doc is open).
   *  `undefined` (no document loaded) hides the button. */
  onExtract?: (() => void) | undefined;
  /** SPEC: P2-PAGE-007 — open the split dialog (when a doc is open).
   *  `undefined` (no document loaded) hides the button. */
  onSplit?: (() => void) | undefined;
  /** SPEC: P2-PAGE-008 — open the merge dialog (when a doc is open).
   *  `undefined` (no document loaded) hides the button. */
  onMerge?: (() => void) | undefined;
  /** SPEC: P2-PAGE-005 — open the insert-from-PDF dialog (when a doc is open).
   *  `undefined` (no document loaded) hides the button. */
  onInsertFromPdf?: (() => void) | undefined;
  /** SPEC: P4-EDIT-009 — open the watermark dialog (when a doc is open).
   *  `undefined` (no document loaded) hides the button. */
  onWatermark?: (() => void) | undefined;
  /** SPEC: P4-EDIT-008 — open the background dialog (when a doc is open).
   *  `undefined` (no document loaded) hides the button. */
  onBackground?: (() => void) | undefined;
  /** SPEC: P4-EDIT-010 — open the header/footer dialog (when a doc is open).
   *  `undefined` (no document loaded) hides the button. */
  onHeaderFooter?: (() => void) | undefined;
}

export function ZoomToolbar({
  onExtract,
  onSplit,
  onMerge,
  onInsertFromPdf,
  onWatermark,
  onBackground,
  onHeaderFooter,
}: ZoomToolbarProps = {}) {
  const zoom = useViewStore((s) => s.zoom);
  const fitMode = useViewStore((s) => s.fitMode);
  const setZoom = useViewStore((s) => s.setZoom);
  const setFitMode = useViewStore((s) => s.setFitMode);
  const showOutline = useViewStore((s) => s.showOutline);
  const toggleOutline = useViewStore((s) => s.toggleOutline);
  const showThumbnails = useViewStore((s) => s.showThumbnails);
  const toggleThumbnails = useViewStore((s) => s.toggleThumbnails);
  const showAnnotations = useViewStore((s) => s.showAnnotations);
  const toggleAnnotations = useViewStore((s) => s.toggleAnnotations);

  // SPEC: P1-VIEW-010 — light / dark / system theme. "Dark" inverts the
  // page for night reading; this control lets the user choose instead of
  // being forced to follow the OS.
  const theme = useSettingsStore((s) => s.theme);
  const setTheme = useSettingsStore((s) => s.setTheme);

  const fitOptions: { value: FitMode | "manual"; label: string }[] = [
    { value: "manual", label: "Manual" },
    { value: "actual", label: "Actual size" },
    { value: "fit-page", label: "Fit page" },
    { value: "fit-width", label: "Fit width" },
    { value: "fit-height", label: "Fit height" },
  ];

  const themeOptions: { value: Theme; label: string }[] = [
    { value: "system", label: "System" },
    { value: "light", label: "Light" },
    { value: "dark", label: "Dark" },
  ];

  const selectValue: FitMode | "manual" = fitMode ?? "manual";

  return (
    <div className="flex flex-wrap items-center gap-x-2 gap-y-1 border-b border-neutral-200 px-3 py-1.5 text-sm dark:border-neutral-800">
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
      <button
        type="button"
        onClick={toggleAnnotations}
        aria-label="Toggle annotations sidebar"
        aria-pressed={showAnnotations}
        title="Toggle annotations list"
        className={
          "rounded px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800 " +
          (showAnnotations ? "bg-neutral-200 dark:bg-neutral-800" : "")
        }
      >
        Annotations
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

      {onExtract ? (
        <button
          type="button"
          onClick={onExtract}
          title="Extract pages to a new PDF"
          className="ml-2 rounded px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800"
        >
          Extract…
        </button>
      ) : null}

      {onSplit ? (
        <button
          type="button"
          onClick={onSplit}
          title="Split into multiple PDFs"
          className="rounded px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800"
        >
          Split…
        </button>
      ) : null}

      {onMerge ? (
        <button
          type="button"
          onClick={onMerge}
          title="Merge PDFs into a new file"
          className="rounded px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800"
        >
          Merge…
        </button>
      ) : null}

      {onInsertFromPdf ? (
        <button
          type="button"
          onClick={onInsertFromPdf}
          title="Insert pages from another PDF"
          className="rounded px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800"
        >
          Insert PDF…
        </button>
      ) : null}

      {onWatermark ? (
        <button
          type="button"
          onClick={onWatermark}
          title="Add a text or image watermark"
          className="rounded px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800"
        >
          Watermark…
        </button>
      ) : null}

      {onBackground ? (
        <button
          type="button"
          onClick={onBackground}
          title="Add a colour or image background behind content"
          className="rounded px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800"
        >
          Background…
        </button>
      ) : null}

      {onHeaderFooter ? (
        <button
          type="button"
          onClick={onHeaderFooter}
          title="Add a header or footer with page-number / date placeholders"
          className="rounded px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800"
        >
          Header/Footer…
        </button>
      ) : null}

      <span className="ml-auto text-neutral-300 dark:text-neutral-700">|</span>
      <label className="flex items-center gap-1" title="Theme">
        <span className="text-neutral-500 dark:text-neutral-400">Theme</span>
        <select
          value={theme}
          onChange={(e) => setTheme(e.target.value as Theme)}
          className="rounded border border-neutral-300 bg-transparent px-1 py-0.5 dark:border-neutral-700"
          aria-label="Theme"
        >
          {themeOptions.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
      </label>
    </div>
  );
}
