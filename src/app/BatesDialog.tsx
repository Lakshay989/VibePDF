// SPEC: P4-EDIT-012 (P4.D5) — stamp a Bates id ({prefix}{padded seq}{suffix}) on
// every page of the open document, gap-free (no exclusions — a Bates id must be
// unique and consecutive). The dialog calls the backend directly and bumps the
// edit epoch so the canvas re-renders. Controlled component: the parent owns `open`.

import * as Dialog from "@radix-ui/react-dialog";
import { useEffect, useState } from "react";

import { addBates } from "@/ipc/bates";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useHistoryStore } from "@/state/history-store";

export interface BatesDialogProps {
  open: boolean;
  documentId: string;
  pageCount: number;
  onClose: () => void;
}

const FONTS = ["Helvetica", "Times", "Courier"] as const;

/** The label the backend will draw for a given number — mirrors `bates_label`. */
function preview(prefix: string, suffix: string, padding: number, value: number): string {
  const digits = String(Math.max(0, Math.trunc(value)));
  return `${prefix}${digits.padStart(Math.max(0, padding), "0")}${suffix}`;
}

export function BatesDialog({ open, documentId, pageCount, onClose }: BatesDialogProps) {
  const setHistory = useHistoryStore((s) => s.setHistory);
  const bumpEpoch = useEditEpochStore((s) => s.bumpEpoch);

  const [position, setPosition] = useState<"header" | "footer">("footer");
  const [align, setAlign] = useState<"left" | "center" | "right">("right");
  const [prefix, setPrefix] = useState("");
  const [suffix, setSuffix] = useState("");
  const [padding, setPadding] = useState("6");
  const [start, setStart] = useState("1");
  const [fontFamily, setFontFamily] = useState<(typeof FONTS)[number]>("Helvetica");
  const [fontSize, setFontSize] = useState("10");
  const [color, setColor] = useState("#000000");
  const [margin, setMargin] = useState("36");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (open) {
      setPosition("footer");
      setAlign("right");
      setPrefix("");
      setSuffix("");
      setPadding("6");
      setStart("1");
      setError(null);
      setBusy(false);
    }
  }, [open]);

  const apply = () => {
    const startNum = Number(start);
    if (start.trim() === "" || !Number.isInteger(startNum) || startNum < 0) {
      setError("Starting number must be a whole number of zero or more.");
      return;
    }
    const padNum = Number(padding);
    if (padding.trim() === "" || !Number.isInteger(padNum) || padNum < 0) {
      setError("Padding must be a whole number of zero or more (use 0 for none).");
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
    addBates(documentId, {
      position,
      align,
      prefix,
      suffix,
      padding: padNum,
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

  const startNum = Number(start);
  const padNum = Number(padding);
  const first = preview(prefix, suffix, padNum, Number.isFinite(startNum) ? startNum : 0);
  const last = preview(
    prefix,
    suffix,
    padNum,
    (Number.isFinite(startNum) ? startNum : 0) + Math.max(0, pageCount - 1),
  );

  return (
    <Dialog.Root open={open} onOpenChange={(next) => !next && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-black/40" />
        <Dialog.Content className="fixed left-1/2 top-1/2 w-[460px] max-w-[92%] -translate-x-1/2 -translate-y-1/2 rounded-lg bg-white p-5 shadow-xl dark:bg-neutral-900 dark:text-neutral-100">
          <Dialog.Title className="text-base font-semibold">Bates numbering</Dialog.Title>
          <Dialog.Description className="mt-1 text-sm text-neutral-600 dark:text-neutral-400">
            Stamps a unique, consecutive id on every page. This document has {pageCount} page
            {pageCount === 1 ? "" : "s"}.
          </Dialog.Description>

          <form
            className="mt-4 flex flex-col gap-3"
            onSubmit={(e) => {
              e.preventDefault();
              apply();
            }}
          >
            <div className="flex items-center gap-2 text-sm">
              <input
                value={prefix}
                onChange={(e) => setPrefix(e.target.value)}
                placeholder="Prefix (e.g. ABC)"
                aria-label="Prefix"
                className={`flex-1 ${inputCls}`}
              />
              <input
                value={suffix}
                onChange={(e) => setSuffix(e.target.value)}
                placeholder="Suffix"
                aria-label="Suffix"
                className={`flex-1 ${inputCls}`}
              />
            </div>

            <div className="flex items-center gap-3 text-sm">
              <label className="flex items-center gap-2">
                <span className="text-neutral-600 dark:text-neutral-400">Start at</span>
                <input
                  type="number"
                  min={0}
                  value={start}
                  onChange={(e) => setStart(e.target.value)}
                  aria-label="Starting number"
                  className={`w-20 ${inputCls}`}
                />
              </label>
              <label className="flex items-center gap-2">
                <span className="text-neutral-600 dark:text-neutral-400">Digits</span>
                <input
                  type="number"
                  min={0}
                  value={padding}
                  onChange={(e) => setPadding(e.target.value)}
                  aria-label="Padding digits"
                  className={`w-16 ${inputCls}`}
                />
              </label>
            </div>

            <p className="text-xs text-neutral-500 dark:text-neutral-400">
              Preview: <code>{first}</code>
              {pageCount > 1 ? (
                <>
                  {" … "}
                  <code>{last}</code>
                </>
              ) : null}
            </p>

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
              <select
                value={align}
                onChange={(e) => setAlign(e.target.value as "left" | "center" | "right")}
                aria-label="Alignment"
                className={`ml-auto ${inputCls}`}
              >
                <option value="left">Left</option>
                <option value="center">Center</option>
                <option value="right">Right</option>
              </select>
            </div>

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
              <label className="flex items-center gap-1">
                <span className="text-neutral-600 dark:text-neutral-400">Margin</span>
                <input
                  type="number"
                  min={0}
                  value={margin}
                  onChange={(e) => setMargin(e.target.value)}
                  aria-label="Margin"
                  className={`w-16 ${inputCls}`}
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
