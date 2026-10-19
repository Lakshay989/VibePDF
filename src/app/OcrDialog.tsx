// SPEC: P7-OCR-001 — read the text in a scanned document and add it as an
// invisible layer, so the page can be searched and selected.
//
// Also the only surface for two other spec lines the engine already satisfies:
// P7-OCR-002 (which language, and adding a thirteenth) and P7-OCR-003 (the
// preprocessing switches, which existed with nothing to set them).
//
// Two honesty decisions worth keeping:
//
//   - **No progress bar.** A run reports nothing until it finishes, so a bar
//     would be animation, not information. The dialog says how long it is
//     likely to take — from a measurement, ~4.5 s a page — and disables itself.
//   - **Downloads start off.** Fetching a language pack is the application's
//     only network call. The switch says plainly what turning it on allows,
//     and everything else here works with the network off.

import { useEffect, useState } from "react";

import { reportError } from "@/app/report-error";
import {
  downloadsAllowed as readDownloadsAllowed,
  installLanguageFile,
  type LanguagePack,
  listLanguages,
  removeLanguage,
  setDownloadsAllowed,
} from "@/ipc/languages";
import { type OcrSummary, runOcr } from "@/ipc/ocr";
import type { DocumentId } from "@/ipc/pdf";
import { useEditEpochStore } from "@/state/edit-epoch-store";

interface Props {
  open: boolean;
  documentId: DocumentId;
  /** 0-based page the viewer is showing, for the "this page only" choice. */
  currentPage: number;
  pageCount: number;
  onClose: () => void;
}

/** SPEC: P7-OCR-003 — the pipeline's three stages, in plain words. */
const QUALITY: ReadonlyArray<{
  key: "deskew" | "denoise" | "upscale";
  label: string;
  detail: string;
}> = [
  {
    key: "deskew",
    label: "Straighten crooked pages",
    detail: "Finds the tilt of a page fed in at an angle and reads it level.",
  },
  {
    key: "denoise",
    label: "Remove specks",
    detail: "Drops isolated dots left by dust or a dirty scanner glass.",
  },
  {
    key: "upscale",
    label: "Enlarge low-resolution pages",
    detail: "Scans below 300 DPI are enlarged first, which is where most of the accuracy is.",
  },
];

/** Seconds per page, measured on a 300 DPI page — see steps/P7.md. */
const SECONDS_PER_PAGE = 4.5;

export function OcrDialog({ open, documentId, currentPage, pageCount, onClose }: Props) {
  const [languages, setLanguages] = useState<LanguagePack[]>([]);
  const [language, setLanguage] = useState("eng");
  const [wholeDocument, setWholeDocument] = useState(true);
  const [quality, setQuality] = useState({ deskew: true, denoise: true, upscale: true });
  const [downloads, setDownloads] = useState(false);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<OcrSummary | null>(null);
  const bumpEpoch = useEditEpochStore((s) => s.bumpEpoch);

  useEffect(() => {
    if (!open) return;
    void (async () => {
      try {
        setLanguages(await listLanguages());
        setDownloads(await readDownloadsAllowed());
      } catch (err) {
        reportError("Couldn't read the OCR languages", err);
      }
    })();
  }, [open]);

  if (!open) return null;

  const pages = wholeDocument ? [] : [currentPage];
  const pageTotal = wholeDocument ? pageCount : 1;
  const estimate = Math.max(1, Math.round(pageTotal * SECONDS_PER_PAGE));

  const close = () => {
    setDone(null);
    onClose();
  };

  const start = () => {
    void (async () => {
      setBusy(true);
      setDone(null);
      try {
        const reply = await runOcr(documentId, pages, {
          language,
          deskew: quality.deskew,
          denoise: quality.denoise,
          // The engine takes a DPI floor; the switch is the human version of it.
          minDpi: quality.upscale ? 300 : 0,
        });
        // Kept open, showing what was read: the page looks identical
        // afterwards, so closing straight away would leave no evidence that
        // anything happened.
        setDone(reply.summary);
        bumpEpoch(documentId);
      } catch (err) {
        reportError("Couldn't read this document", err);
      } finally {
        setBusy(false);
      }
    })();
  };

  const addLanguageFile = () => {
    void (async () => {
      try {
        const { open: openFileDialog } = await import("@tauri-apps/plugin-dialog");
        const picked = await openFileDialog({
          multiple: false,
          filters: [{ name: "Tesseract language data", extensions: ["traineddata"] }],
        });
        if (typeof picked !== "string") return;
        const pack = await installLanguageFile(picked);
        setLanguages(await listLanguages());
        setLanguage(pack.code);
      } catch (err) {
        reportError("Couldn't add that language", err);
      }
    })();
  };

  const drop = (code: string) => {
    void (async () => {
      try {
        await removeLanguage(code);
        setLanguages(await listLanguages());
        if (language === code) setLanguage("eng");
      } catch (err) {
        reportError("Couldn't remove that language", err);
      }
    })();
  };

  const toggleDownloads = (allowed: boolean) => {
    void (async () => {
      try {
        setDownloads(await setDownloadsAllowed(allowed));
      } catch (err) {
        reportError("Couldn't change that setting", err);
      }
    })();
  };

  return (
    <div
      role="dialog"
      aria-label="Read text in this document"
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/40"
    >
      <div className="w-[560px] rounded-lg bg-white p-4 shadow-xl dark:bg-neutral-900">
        <h2 className="mb-1 text-sm font-medium">Read text (OCR)</h2>
        <p className="mb-3 text-xs text-neutral-500">
          Reads the words in a scanned page and adds them as invisible text, so the document can
          be searched and its text selected. The page itself is not changed.
        </p>

        <label className="mb-3 block text-xs">
          <span className="mb-1 block text-neutral-500">Language</span>
          <select
            value={language}
            onChange={(e) => setLanguage(e.target.value)}
            disabled={busy}
            className="w-full rounded border px-2 py-1 dark:bg-neutral-800"
          >
            {languages.map((pack) => (
              <option key={pack.code} value={pack.code}>
                {pack.name}
                {pack.source === "added" ? " (added)" : ""}
              </option>
            ))}
          </select>
        </label>

        <fieldset className="mb-3 text-xs" disabled={busy}>
          <legend className="mb-1 text-neutral-500">Pages</legend>
          <label className="mr-3">
            <input
              type="radio"
              name="ocr-pages"
              checked={wholeDocument}
              onChange={() => setWholeDocument(true)}
            />{" "}
            Whole document ({pageCount} {pageCount === 1 ? "page" : "pages"})
          </label>
          <label>
            <input
              type="radio"
              name="ocr-pages"
              checked={!wholeDocument}
              onChange={() => setWholeDocument(false)}
            />{" "}
            This page only
          </label>
        </fieldset>

        <fieldset className="mb-3 text-xs" disabled={busy}>
          <legend className="mb-1 text-neutral-500">Quality</legend>
          {QUALITY.map(({ key, label, detail }) => (
            <label key={key} className="mb-1 flex gap-2">
              <input
                type="checkbox"
                checked={quality[key]}
                onChange={(e) => setQuality({ ...quality, [key]: e.target.checked })}
              />
              <span>
                {label}
                <span className="block text-neutral-500">{detail}</span>
              </span>
            </label>
          ))}
        </fieldset>

        <details className="mb-3 text-xs">
          <summary className="cursor-pointer text-neutral-500">Manage languages</summary>
          <div className="mt-2 space-y-2">
            <ul className="max-h-28 overflow-y-auto">
              {languages.map((pack) => (
                <li key={pack.code} className="flex items-center justify-between py-0.5">
                  <span>
                    {pack.name} <span className="text-neutral-500">({pack.code})</span>
                  </span>
                  {pack.source === "added" ? (
                    <button
                      type="button"
                      onClick={() => drop(pack.code)}
                      className="rounded px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800"
                    >
                      Remove
                    </button>
                  ) : (
                    <span className="text-neutral-500">included</span>
                  )}
                </li>
              ))}
            </ul>
            <button
              type="button"
              onClick={addLanguageFile}
              className="rounded border px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800"
            >
              Add a language file…
            </button>
            <label className="flex gap-2">
              <input
                type="checkbox"
                checked={downloads}
                onChange={(e) => toggleDownloads(e.target.checked)}
              />
              <span>
                Allow downloading language packs
                <span className="block text-neutral-500">
                  Lets VibePDF fetch a language you ask for from the internet. Everything else,
                  including the {languages.filter((p) => p.source === "bundled").length} languages
                  above, works offline.
                </span>
              </span>
            </label>
          </div>
        </details>

        {done ? (
          <p className="mb-3 rounded bg-neutral-100 p-2 text-xs dark:bg-neutral-800">
            Read {done.words} {done.words === 1 ? "word" : "words"} on {done.pages}{" "}
            {done.pages === 1 ? "page" : "pages"}. The text is now searchable; Undo reverses this.
            {done.words === 0
              ? " Nothing was readable — this may not be a scanned page, or it may be in another language."
              : ""}
          </p>
        ) : (
          <p className="mb-3 text-xs text-neutral-500">
            {busy
              ? `Reading ${pageTotal} ${pageTotal === 1 ? "page" : "pages"}. This takes about ${estimate} seconds and cannot be interrupted.`
              : `About ${estimate} seconds for ${pageTotal} ${pageTotal === 1 ? "page" : "pages"}.`}
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
            disabled={busy || languages.length === 0}
            className="rounded bg-blue-600 px-3 py-1 text-xs text-white disabled:opacity-50"
          >
            {busy ? "Reading…" : done ? "Read again" : "Read text"}
          </button>
        </div>
      </div>
    </div>
  );
}
