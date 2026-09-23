// SPEC: P7-OCR-010 — make a PDF smaller, at one of three levels, and say by
// how much.
//
// Three decisions worth keeping:
//
//   - **It saves a copy; it never overwrites.** Image recompression is lossy.
//     If this were an edit on the open document, the next Ctrl-S would replace
//     the user's original with the degraded version. Keeping the original is
//     the only way "try high and see if it still looks right" is a safe thing
//     to suggest.
//   - **The levels are described by what they cost, not by their names.**
//     "High" tells a user nothing; "about 1% of the original, photographs
//     soften" tells them whether to pick it. The percentages are measured, not
//     promised — see `pdf::compress`.
//   - **No progress bar.** The backend reports nothing until it finishes, as
//     with OCR and image export, so the dialog states the expected duration
//     instead of animating a guess.

import { useState } from "react";

import { reportError } from "@/app/report-error";
import { type CompressLevel, compressDocument, type CompressReport } from "@/ipc/compress";
import type { DocumentId } from "@/ipc/pdf";

interface Props {
  open: boolean;
  documentId: DocumentId;
  /** Suggested output name, derived from the document's own file name. */
  suggestedName: string;
  /** Size of the file on disk, so the dialog can estimate before it runs.
   *  Zero when it could not be read — the estimate is then simply omitted
   *  rather than shown as "0 bytes now". */
  currentBytes: number;
  pageCount: number;
  onClose: () => void;
}

/**
 * SPEC: P7-OCR-010 — the three levels in the user's terms.
 *
 * `share` is the measured output size as a fraction of a 300 DPI scan — the
 * document type this feature exists for. It is used for the estimate and
 * labelled as approximate, because a text PDF compresses nothing like a scan.
 */
const LEVELS: ReadonlyArray<{
  key: CompressLevel;
  label: string;
  detail: string;
  share: number;
}> = [
  {
    key: "low",
    label: "Smaller",
    detail: "Keeps every pixel. Nothing changes that you could see at any zoom.",
    share: 0.23,
  },
  {
    key: "medium",
    label: "Much smaller",
    detail: "Reduces scans to 200 DPI. Nothing visible at normal reading size.",
    share: 0.065,
  },
  {
    key: "high",
    label: "Smallest",
    detail: "Reduces scans to 150 DPI. Text stays sharp; photographs soften.",
    share: 0.007,
  },
];

/**
 * Measured 2026-09-23 on a release build: 6 pages of a 300 DPI scan took
 * 0.52-0.78 s depending on level, so about an eighth of a second a page. A
 * quarter is the honest rounding — the machine varies, and a number that is
 * slightly pessimistic is the right kind of wrong for a wait.
 */
const SECONDS_PER_PAGE = 0.25;

export function CompressDialog({
  open,
  documentId,
  suggestedName,
  currentBytes,
  pageCount,
  onClose,
}: Props) {
  const [level, setLevel] = useState<CompressLevel>("medium");
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<CompressReport | null>(null);

  if (!open) return null;

  const chosen = LEVELS.find((l) => l.key === level) ?? LEVELS[1];
  const estimate = Math.max(1, Math.round(pageCount * SECONDS_PER_PAGE));

  const close = () => {
    setDone(null);
    onClose();
  };

  const start = () => {
    void (async () => {
      setBusy(true);
      setDone(null);
      try {
        const { save: saveDialog } = await import("@tauri-apps/plugin-dialog");
        const dest = await saveDialog({
          defaultPath: suggestedName,
          filters: [{ name: "PDF", extensions: ["pdf"] }],
        });
        if (typeof dest !== "string") return; // user cancelled the dialog
        setDone(await compressDocument(documentId, dest, level));
      } catch (err) {
        reportError("Couldn't compress this document", err);
      } finally {
        setBusy(false);
      }
    })();
  };

  return (
    <div
      role="dialog"
      aria-label="Compress this document"
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/40"
    >
      <div className="w-[560px] rounded-lg bg-white p-4 shadow-xl dark:bg-neutral-900">
        <h2 className="mb-1 text-sm font-medium">Compress</h2>
        <p className="mb-3 text-xs text-neutral-500">
          Saves a smaller copy. The document you have open is not changed, so the original is
          always still there if the copy is not good enough.
        </p>

        <fieldset className="mb-3 text-xs" disabled={busy}>
          <legend className="mb-1 text-neutral-500">How small</legend>
          {LEVELS.map(({ key, label, detail }) => (
            <label key={key} className="mb-1 flex gap-2">
              <input
                type="radio"
                name="compress-level"
                checked={level === key}
                onChange={() => setLevel(key)}
              />
              <span>
                {label}
                <span className="block text-neutral-500">{detail}</span>
              </span>
            </label>
          ))}
        </fieldset>

        {done ? (
          <p className="mb-3 rounded bg-neutral-100 p-2 text-xs dark:bg-neutral-800">
            {done.afterBytes < done.beforeBytes ? (
              <>
                {formatBytes(done.beforeBytes)} → <strong>{formatBytes(done.afterBytes)}</strong> (
                {percent(done.afterBytes / done.beforeBytes)} of the original,{" "}
                {formatBytes(done.beforeBytes - done.afterBytes)} saved).{" "}
                {done.imagesRecompressed > 0
                  ? `${done.imagesRecompressed} ${done.imagesRecompressed === 1 ? "image" : "images"} recompressed. `
                  : ""}
                {done.streamsDeflated > 0 ? `${done.streamsDeflated} streams deflated.` : ""}
              </>
            ) : (
              <>
                This document was already as small as we can make it, so the copy is identical to
                the original. Nothing was degraded.
              </>
            )}
          </p>
        ) : (
          <p className="mb-3 text-xs text-neutral-500">
            {busy
              ? `Compressing ${pageCount} ${pageCount === 1 ? "page" : "pages"}. This takes about ${estimate} seconds and cannot be interrupted.`
              : currentBytes > 0
                ? `${formatBytes(currentBytes)} now; roughly ${formatBytes(currentBytes * chosen.share)} for a scanned document. A document that is mostly text will save less, because there is less to throw away.`
                : `About ${estimate} seconds for ${pageCount} ${pageCount === 1 ? "page" : "pages"}.`}
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
            disabled={busy}
            className="rounded bg-blue-600 px-3 py-1 text-xs text-white disabled:opacity-50"
          >
            {busy ? "Compressing…" : done ? "Compress again" : "Save a copy…"}
          </button>
        </div>
      </div>
    </div>
  );
}

/** Sizes people read, not bytes people count. */
function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${Math.round(bytes)} bytes`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/** A share of the original, never rounded down to a misleading "0%". */
function percent(share: number): string {
  const value = share * 100;
  return value < 1 ? "under 1%" : `${Math.round(value)}%`;
}
