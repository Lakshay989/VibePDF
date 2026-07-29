// SPEC: P4-EDIT-011 (P4.D4) — stamp page numbers on a document. The number on
// the page at 0-based index i is `start + i`; excluded pages are not stamped but
// don't shift the sequence. Every format is ASCII, so this never needs the
// embedded-font path. The dialog calls the backend directly and bumps the edit
// epoch so the canvas re-renders. Controlled component: the parent owns `open`.

import * as Dialog from "@radix-ui/react-dialog";
import { useEffect, useState } from "react";

import { addPageNumbers, type NumberFormat } from "@/ipc/page-numbers";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useHistoryStore } from "@/state/history-store";
import { parsePageRange } from "@/tools/page-range";

export interface PageNumbersDialogProps {
  open: boolean;
  documentId: string;
  pageCount: number;
  onClose: () => void;
}

const FONTS = ["Helvetica", "Times", "Courier"] as const;

/** Format options + a static illustrative preview (no numeral logic duplicated). */
const FORMATS: { value: NumberFormat; label: string; example: string }[] = [
  { value: "decimal", label: "1, 2, 3", example: "1, 2, 3" },
  { value: "decimal-slash-total", label: "1/N", example: "1/N, 2/N" },
  { value: "page-x-of-n", label: "Page 1 of N", example: "Page 1 of N" },
  { value: "lower-roman", label: "i, ii, iii", example: "i, ii, iii" },
  { value: "upper-roman", label: "I, II, III", example: "I, II, III" },
  { value: "lower-alpha", label: "a, b, c", example: "a, b, c" },
  { value: "upper-alpha", label: "A, B, C", example: "A, B, C" },
];

export function PageNumbersDialog({ open, documentId, pageCount, onClose }: PageNumbersDialogProps) {
  const setHistory = useHistoryStore((s) => s.setHistory);
  const bumpEpoch = useEditEpochStore((s) => s.bumpEpoch);

  const [position, setPosition] = useState<"header" | "footer">("footer");
  const [align, setAlign] = useState<"left" | "center" | "right">("center");
  const [format, setFormat] = useState<NumberFormat>("decimal");
  const [start, setStart] = useState("1");
  const [fontFamily, setFontFamily] = useState<(typeof FONTS)[number]>("Helvetica");
  const [fontSize, setFontSize] = useState("10");
  const [color, setColor] = useState("#000000");
  const [margin, setMargin] = useState("36");
  const [exclude, setExclude] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (open) {
      setPosition("footer");
      setAlign("center");
      setFormat("decimal");
      setStart("1");
      setExclude("");
      setError(null);
      setBusy(false);
    }
  }, [open]);

  const apply = () => {
    const startNum = Number(start);
    if (!Number.isInteger(startNum) || startNum < 1) {
      setError("Starting number must be a whole number of at least 1.");
      return;
    }
    // Empty exclusion means "exclude nothing" (parsePageRange treats "" as all).
    let excluded: number[] = [];
    if (exclude.trim() !== "") {
      const parsed = parsePageRange(exclude, pageCount);
      if ("error" in parsed) {
        setError(parsed.error);
        return;
      }
      excluded = parsed.pages;
    }
    const size = Number(fontSize);
    const mar = Number(margin);
    if (!Number.isFinite(size) || size <= 0) {
      setError("Font size must be greater than zero.");
      return;
    }
    if (!Number.isFinite(mar) || mar < 0) {
      setError("Margin must be zero or greater.");
      return;
    }

    setBusy(true);
    setError(null);
    addPageNumbers(documentId, excluded, {
      position,
      align,
      format,
      start: startNum,
      fontFamily,
      fontSize: size,
      color,
      margin: mar,
    })
      .then((h) => {
        bumpEpoch(documentId);
        setHistory(documentId, h);
        onClose();
      })
      .catch((err: unknown) => {
        setError(err instanceof Error ? err.message : String(err));
        setBusy(false);
      });
  };

  const inputCls =
    "rounded border border-neutral-300 bg-white px-2 py-1.5 text-sm outline-none focus:border-neutral-500 dark:border-neutral-700 dark:bg-neutral-800";

  return (
    <Dialog.Root open={open} onOpenChange={(next) => !next && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-black/40" />
        <Dialog.Content className="fixed left-1/2 top-1/2 w-[460px] max-w-[92%] -translate-x-1/2 -translate-y-1/2 rounded-lg bg-white p-5 shadow-xl dark:bg-neutral-900 dark:text-neutral-100">
          <Dialog.Title className="text-base font-semibold">Page numbers</Dialog.Title>
          <Dialog.Description className="mt-1 text-sm text-neutral-600 dark:text-neutral-400">
            Numbers every page (the first shows the starting number). This document has{" "}
            {pageCount} page{pageCount === 1 ? "" : "s"}.
          </Dialog.Description>

          <form
            className="mt-4 flex flex-col gap-3"
            onSubmit={(e) => {
              e.preventDefault();
              apply();
            }}
          >
            <div className="flex gap-4 text-sm">
              <label className="flex items-center gap-1.5">
                <input
                  type="radio"
                  checked={position === "header"}
                  onChange={() => setPosition("header")}
                />
                Header
              </label>
              <label className="flex items-center gap-1.5">
                <input
                  type="radio"
                  checked={position === "footer"}
                  onChange={() => setPosition("footer")}
                />
                Footer
              </label>
            </div>

            <div className="flex items-center gap-2 text-sm">
              <select
                value={align}
                onChange={(e) => setAlign(e.target.value as "left" | "center" | "right")}
                aria-label="Alignment"
                className={`flex-1 ${inputCls}`}
              >
                <option value="left">Left</option>
                <option value="center">Center</option>
                <option value="right">Right</option>
              </select>
              <select
                value={format}
                onChange={(e) => setFormat(e.target.value as NumberFormat)}
                aria-label="Format"
                className={`flex-1 ${inputCls}`}
              >
                {FORMATS.map((f) => (
                  <option key={f.value} value={f.value}>
                    {f.label}
                  </option>
                ))}
              </select>
            </div>

            <p className="text-xs text-neutral-500 dark:text-neutral-400">
              Example: {FORMATS.find((f) => f.value === format)?.example}
            </p>

            <div className="flex items-center gap-2 text-sm">
              <select
                value={fontFamily}
                onChange={(e) => setFontFamily(e.target.value as (typeof FONTS)[number])}
                aria-label="Font"
                className={`flex-1 ${inputCls}`}
              >
                {FONTS.map((f) => (
                  <option key={f} value={f}>
                    {f}
                  </option>
                ))}
              </select>
              <input
                type="number"
                min={1}
                value={fontSize}
                onChange={(e) => setFontSize(e.target.value)}
                aria-label="Font size"
                className={`w-16 ${inputCls}`}
              />
              <input
                type="color"
                value={color}
                onChange={(e) => setColor(e.target.value)}
                aria-label="Text color"
                className="h-8 w-10 rounded border border-neutral-300 dark:border-neutral-700"
              />
            </div>

            <div className="flex items-center gap-3 text-sm">
              <label className="flex items-center gap-2">
                <span className="text-neutral-600 dark:text-neutral-400">Start at</span>
                <input
                  type="number"
                  min={1}
                  value={start}
                  onChange={(e) => setStart(e.target.value)}
                  aria-label="Starting number"
                  className={`w-16 ${inputCls}`}
                />
              </label>
              <label className="flex flex-1 items-center gap-2">
                <span className="text-neutral-600 dark:text-neutral-400">Margin</span>
                <input
                  type="number"
                  min={0}
                  value={margin}
                  onChange={(e) => setMargin(e.target.value)}
                  aria-label="Margin"
                  className={`w-20 ${inputCls}`}
                />
              </label>
            </div>

            <label className="flex items-center gap-2 text-sm">
              <span className="text-neutral-600 dark:text-neutral-400">Skip pages</span>
              <input
                value={exclude}
                onChange={(e) => setExclude(e.target.value)}
                placeholder="none, or 1, 3-4"
                aria-label="Skip pages"
                className={`flex-1 ${inputCls}`}
              />
            </label>

            {error ? (
              <div role="alert" className="text-sm text-red-600 dark:text-red-400">
                {error}
              </div>
            ) : null}

            <div className="mt-1 flex justify-end gap-2">
              <button
                type="button"
                onClick={onClose}
                className="rounded border border-neutral-300 px-3 py-1.5 text-sm hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
              >
                Cancel
              </button>
              <button
                type="submit"
                disabled={busy}
                className="rounded bg-neutral-900 px-3 py-1.5 text-sm text-white hover:bg-neutral-800 disabled:opacity-50 dark:bg-neutral-100 dark:text-neutral-900 dark:hover:bg-neutral-200"
              >
                {busy ? "Applying…" : "Apply"}
              </button>
            </div>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
