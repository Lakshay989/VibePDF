// SPEC: P4-EDIT-010 (P4.D3) — add a header or footer with left / centre / right
// text on selected pages. `{n}` / `{total}` / `{date}` placeholders are
// substituted per page (the date is this frontend's locale-formatted today). The
// dialog calls the backend directly and bumps the edit epoch so the canvas
// re-renders. Controlled component: the parent owns `open`.

import * as Dialog from "@radix-ui/react-dialog";
import { useEffect, useState } from "react";

import { addHeaderFooter } from "@/ipc/header-footer";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useHistoryStore } from "@/state/history-store";
import { parsePageRange } from "@/tools/page-range";

export interface HeaderFooterDialogProps {
  open: boolean;
  documentId: string;
  pageCount: number;
  onClose: () => void;
}

const FONTS = ["Helvetica", "Times", "Courier"] as const;

/** Today as `YYYY-MM-DD` in local time — the `{date}` value. */
function today(): string {
  const d = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

export function HeaderFooterDialog({ open, documentId, pageCount, onClose }: HeaderFooterDialogProps) {
  const setHistory = useHistoryStore((s) => s.setHistory);
  const bumpEpoch = useEditEpochStore((s) => s.bumpEpoch);

  const [position, setPosition] = useState<"header" | "footer">("header");
  const [left, setLeft] = useState("");
  const [center, setCenter] = useState("");
  const [right, setRight] = useState("");
  const [fontFamily, setFontFamily] = useState<(typeof FONTS)[number]>("Helvetica");
  const [fontSize, setFontSize] = useState("10");
  const [color, setColor] = useState("#000000");
  const [margin, setMargin] = useState("36");
  const [range, setRange] = useState("all");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (open) {
      setPosition("header");
      setLeft("");
      setCenter("");
      setRight("");
      setRange("all");
      setError(null);
      setBusy(false);
    }
  }, [open]);

  const apply = () => {
    if (left.trim() === "" && center.trim() === "" && right.trim() === "") {
      setError("Enter text for at least one position.");
      return;
    }
    const parsed = parsePageRange(range, pageCount);
    if ("error" in parsed) {
      setError(parsed.error);
      return;
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
    addHeaderFooter(documentId, parsed.pages, {
      position,
      left,
      center,
      right,
      fontFamily,
      fontSize: size,
      color,
      margin: mar,
      date: today(),
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
          <Dialog.Title className="text-base font-semibold">Header / footer</Dialog.Title>
          <Dialog.Description className="mt-1 text-sm text-neutral-600 dark:text-neutral-400">
            Placeholders: <code>{"{n}"}</code> page number, <code>{"{total}"}</code> total pages,{" "}
            <code>{"{date}"}</code> today. This document has {pageCount} page
            {pageCount === 1 ? "" : "s"}.
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

            <input
              value={left}
              onChange={(e) => setLeft(e.target.value)}
              placeholder="Left"
              aria-label="Left text"
              className={inputCls}
            />
            <input
              value={center}
              onChange={(e) => setCenter(e.target.value)}
              placeholder="Center, e.g. Page {n} of {total}"
              aria-label="Center text"
              className={inputCls}
            />
            <input
              value={right}
              onChange={(e) => setRight(e.target.value)}
              placeholder="Right, e.g. {date}"
              aria-label="Right text"
              className={inputCls}
            />

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
              <label className="flex flex-1 items-center gap-2">
                <span className="text-neutral-600 dark:text-neutral-400">Pages</span>
                <input
                  value={range}
                  onChange={(e) => setRange(e.target.value)}
                  placeholder="all, or 1-3, 5"
                  aria-label="Pages"
                  className={`flex-1 ${inputCls}`}
                />
              </label>
            </div>

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
