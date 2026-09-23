// SPEC: P7-OCR-005 — export pages as images: PNG, JPG, TIFF or WebP, at a DPI
// the user chooses between 72 and 600.
//
// Three decisions worth keeping:
//
//   - **The DPI field is a number, not a preset list.** The spec gives a range,
//     and a scanner operator who wants 203 DPI should be able to type it. The
//     common values sit in a datalist, so the list is a shortcut, not a fence.
//   - **The estimate is pixels, not seconds.** Unlike OCR, the cost here is
//     dominated by output size, which we can state exactly before the run:
//     a 600-DPI page is 5100 × 6600, and people are surprised by that.
//   - **No progress bar.** The backend reports nothing until the run finishes,
//     the same as OCR, so the dialog disables itself and says how many files
//     are coming.

import { useState } from "react";

import { reportError } from "@/app/report-error";
import {
  exportImages,
  type ImageExportFormat,
  type ImageExportSummary,
  MAX_DPI,
  MIN_DPI,
} from "@/ipc/export-image";
import type { DocumentId } from "@/ipc/pdf";

interface Props {
  open: boolean;
  documentId: DocumentId;
  /** 0-based page the viewer is showing, for the "this page only" choice. */
  currentPage: number;
  pageCount: number;
  /** Width and height of the current page in points, for the pixel estimate. */
  pageWidthPoints: number;
  pageHeightPoints: number;
  /** File stem for the output, derived from the document's own name. */
  stem: string;
  onClose: () => void;
}

/** SPEC: P7-OCR-005 — the four formats, in plain words. */
const FORMATS: ReadonlyArray<{
  key: ImageExportFormat;
  label: string;
  detail: string;
}> = [
  {
    key: "png",
    label: "PNG",
    detail: "Lossless. The safe choice for text and line art.",
  },
  {
    key: "jpeg",
    label: "JPG",
    detail: "Lossy but small. Best for pages that are mostly photographs.",
  },
  {
    key: "tiff",
    label: "TIFF",
    detail: "Lossless, widely accepted by archives and print shops.",
  },
  {
    key: "webp",
    label: "WebP",
    detail: "Lossless, smaller than PNG. Not every older program opens it.",
  },
];

/** JPEG quality, as three choices rather than a slider nobody can calibrate. */
const QUALITY: ReadonlyArray<{ label: string; value: number; detail: string }> = [
  { label: "Smaller file", value: 60, detail: "Visible softening on text." },
  { label: "Balanced", value: 85, detail: "No artefacts at 100% zoom." },
  { label: "Best quality", value: 95, detail: "Close to lossless, much larger." },
];

export function ExportImageDialog({
  open,
  documentId,
  currentPage,
  pageCount,
  pageWidthPoints,
  pageHeightPoints,
  stem,
  onClose,
}: Props) {
  const [format, setFormat] = useState<ImageExportFormat>("png");
  const [dpi, setDpi] = useState(150);
  const [quality, setQuality] = useState(85);
  const [wholeDocument, setWholeDocument] = useState(true);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<ImageExportSummary | null>(null);

  if (!open) return null;

  const pages = wholeDocument ? [] : [currentPage];
  const pageTotal = wholeDocument ? pageCount : 1;
  // Clearing the field makes `valueAsNumber` NaN. No guard is needed for it:
  // every comparison against NaN is false, so the range check below rejects it,
  // and React renders a NaN `value` as an empty field rather than the string
  // "NaN" (verified 2026-09-23 — both guards were tried and neither changed a
  // single rendered character). The test pins that behaviour.
  const dpiValid = dpi >= MIN_DPI && dpi <= MAX_DPI;
  // The same arithmetic the renderer does: pixels = points / 72 * dpi.
  const pixelWidth = Math.round((pageWidthPoints / 72) * dpi);
  const pixelHeight = Math.round((pageHeightPoints / 72) * dpi);

  const close = () => {
    setDone(null);
    onClose();
  };

  const start = () => {
    void (async () => {
      setBusy(true);
      setDone(null);
      try {
        const { open: openFolderDialog } = await import("@tauri-apps/plugin-dialog");
        const destDir = await openFolderDialog({ directory: true, multiple: false });
        if (typeof destDir !== "string") return; // user cancelled the dialog
        setDone(await exportImages(documentId, destDir, stem, pages, { format, dpi, quality }));
      } catch (err) {
        reportError("Couldn't export the images", err);
      } finally {
        setBusy(false);
      }
    })();
  };

  return (
    <div
      role="dialog"
      aria-label="Export pages as images"
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/40"
    >
      <div className="w-[560px] rounded-lg bg-white p-4 shadow-xl dark:bg-neutral-900">
        <h2 className="mb-1 text-sm font-medium">Export as images</h2>
        <p className="mb-3 text-xs text-neutral-500">
          Writes one image file per page into a folder you choose. The PDF itself is not changed.
        </p>

        <fieldset className="mb-3 text-xs" disabled={busy}>
          <legend className="mb-1 text-neutral-500">Format</legend>
          {FORMATS.map(({ key, label, detail }) => (
            <label key={key} className="mb-1 flex gap-2">
              <input
                type="radio"
                name="image-format"
                checked={format === key}
                onChange={() => setFormat(key)}
              />
              <span>
                {label}
                <span className="block text-neutral-500">{detail}</span>
              </span>
            </label>
          ))}
        </fieldset>

        {format === "jpeg" ? (
          <fieldset className="mb-3 text-xs" disabled={busy}>
            <legend className="mb-1 text-neutral-500">Quality</legend>
            {QUALITY.map(({ label, value, detail }) => (
              <label key={value} className="mb-1 flex gap-2">
                <input
                  type="radio"
                  name="image-quality"
                  checked={quality === value}
                  onChange={() => setQuality(value)}
                />
                <span>
                  {label}
                  <span className="block text-neutral-500">{detail}</span>
                </span>
              </label>
            ))}
          </fieldset>
        ) : null}

        <label className="mb-3 block text-xs">
          <span className="mb-1 block text-neutral-500">
            Resolution ({MIN_DPI}–{MAX_DPI} DPI)
          </span>
          <input
            type="number"
            min={MIN_DPI}
            max={MAX_DPI}
            step={1}
            value={dpi}
            list="image-dpi-presets"
            disabled={busy}
            onChange={(e) => setDpi(e.target.valueAsNumber)}
            aria-invalid={!dpiValid}
            className="w-28 rounded border px-2 py-1 dark:bg-neutral-800"
          />
          <datalist id="image-dpi-presets">
            <option value="72" />
            <option value="150" />
            <option value="300" />
            <option value="600" />
          </datalist>
          <span className="ml-2 text-neutral-500">
            {dpiValid
              ? `${pixelWidth} × ${pixelHeight} pixels a page`
              : `Enter a number between ${MIN_DPI} and ${MAX_DPI}.`}
          </span>
        </label>

        <fieldset className="mb-3 text-xs" disabled={busy}>
          <legend className="mb-1 text-neutral-500">Pages</legend>
          <label className="mr-3">
            <input
              type="radio"
              name="image-pages"
              checked={wholeDocument}
              onChange={() => setWholeDocument(true)}
            />{" "}
            Whole document ({pageCount} {pageCount === 1 ? "page" : "pages"})
          </label>
          <label>
            <input
              type="radio"
              name="image-pages"
              checked={!wholeDocument}
              onChange={() => setWholeDocument(false)}
            />{" "}
            This page only
          </label>
        </fieldset>

        {done ? (
          <p className="mb-3 rounded bg-neutral-100 p-2 text-xs dark:bg-neutral-800">
            Wrote {done.pages} {done.pages === 1 ? "file" : "files"} ({formatBytes(done.bytes)}),
            named {done.files[0]}
            {done.files.length > 1 ? ` through ${done.files[done.files.length - 1]}` : ""}.
          </p>
        ) : (
          <p className="mb-3 text-xs text-neutral-500">
            {busy
              ? `Exporting ${pageTotal} ${pageTotal === 1 ? "page" : "pages"}. This cannot be interrupted.`
              : `${pageTotal} ${pageTotal === 1 ? "file" : "files"}, named ${stem}-001.${extensionFor(format)} onwards.`}
          </p>
        )}

        <div className="flex justify-end gap-2">
          <button
            type="button"
            onClick={close}
            disabled={busy}
            className="rounded px-3 py-1 text-xs hover:bg-neutral-100 disabled:opacity-50 dark:hover:bg-neutral-800"
          >
            {done ? "Close" : "Cancel"}
          </button>
          <button
            type="button"
            onClick={start}
            disabled={busy || !dpiValid}
            className="rounded bg-blue-600 px-3 py-1 text-xs text-white disabled:opacity-50"
          >
            {busy ? "Exporting…" : done ? "Export again" : "Choose folder…"}
          </button>
        </div>
      </div>
    </div>
  );
}

/** The extension the backend will use, so the dialog can name the first file. */
function extensionFor(format: ImageExportFormat): string {
  return format === "jpeg" ? "jpg" : format;
}

/** Sizes people read, not bytes people count. */
function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} bytes`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
