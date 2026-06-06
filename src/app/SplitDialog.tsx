// SPEC: P2-PAGE-007 — choose how to split a PDF into multiple files. Four
// modes: every N pages, at specific pages, by file-size target, by top-level
// bookmarks. On confirm the parent opens a directory picker and calls the
// backend. Controlled component: the parent owns `open`.

import * as Dialog from "@radix-ui/react-dialog";
import { useEffect, useState } from "react";

import type { SplitMode } from "@/ipc/split";
import { parseSplitPoints } from "@/tools/split/split-points";

type ModeKind = SplitMode["kind"];

export interface SplitDialogProps {
  open: boolean;
  pageCount: number;
  /** Called with the chosen split mode when the user confirms. */
  onSplit: (mode: SplitMode) => void;
  onClose: () => void;
}

const MODES: { value: ModeKind; label: string }[] = [
  { value: "everyN", label: "Every N pages" },
  { value: "atPages", label: "At specific pages" },
  { value: "bySize", label: "By file size" },
  { value: "bookmarks", label: "By top-level bookmarks" },
];

export function SplitDialog({ open, pageCount, onSplit, onClose }: SplitDialogProps) {
  const [kind, setKind] = useState<ModeKind>("everyN");
  const [everyN, setEveryN] = useState("10");
  const [atPages, setAtPages] = useState("");
  const [sizeMb, setSizeMb] = useState("5");

  useEffect(() => {
    if (open) {
      setKind("everyN");
      setEveryN("10");
      setAtPages("");
      setSizeMb("5");
    }
  }, [open]);

  // Build the SplitMode for the current inputs, or an error to show. Only
  // surface errors once the relevant field has content.
  let mode: SplitMode | null = null;
  let error: string | null = null;
  if (kind === "everyN") {
    const n = Number(everyN);
    if (Number.isInteger(n) && n >= 1 && n < pageCount) {
      mode = { kind: "everyN", n };
    } else if (everyN.trim().length > 0) {
      error = `Pages per file must be between 1 and ${pageCount - 1}.`;
    }
  } else if (kind === "atPages") {
    const parsed = parseSplitPoints(atPages, pageCount);
    if ("boundaries" in parsed) mode = { kind: "atPages", boundaries: parsed.boundaries };
    else if (atPages.trim().length > 0) error = parsed.error;
  } else if (kind === "bySize") {
    const mb = Number(sizeMb);
    if (Number.isFinite(mb) && mb > 0) {
      mode = { kind: "bySize", maxBytes: Math.round(mb * 1024 * 1024) };
    } else if (sizeMb.trim().length > 0) {
      error = "Size must be greater than zero.";
    }
  } else {
    mode = { kind: "bookmarks" };
  }

  return (
    <Dialog.Root
      open={open}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-black/40" />
        <Dialog.Content className="fixed left-1/2 top-1/2 w-[420px] max-w-[92%] -translate-x-1/2 -translate-y-1/2 rounded-lg bg-white p-5 shadow-xl dark:bg-neutral-900 dark:text-neutral-100">
          <Dialog.Title className="text-base font-semibold">Split PDF</Dialog.Title>
          <Dialog.Description className="mt-1 text-sm text-neutral-600 dark:text-neutral-400">
            Split into multiple files. This document has {pageCount} page
            {pageCount === 1 ? "" : "s"}.
          </Dialog.Description>

          <form
            className="mt-4 flex flex-col gap-3"
            onSubmit={(e) => {
              e.preventDefault();
              if (mode) onSplit(mode);
            }}
          >
            <select
              value={kind}
              onChange={(e) => setKind(e.target.value as ModeKind)}
              aria-label="Split mode"
              className="rounded border border-neutral-300 bg-white px-2 py-1.5 text-sm outline-none focus:border-neutral-500 dark:border-neutral-700 dark:bg-neutral-800"
            >
              {MODES.map((m) => (
                <option key={m.value} value={m.value}>
                  {m.label}
                </option>
              ))}
            </select>

            {kind === "everyN" ? (
              <label className="flex items-center gap-2 text-sm">
                <span className="text-neutral-600 dark:text-neutral-400">Pages per file</span>
                <input
                  type="number"
                  min={1}
                  value={everyN}
                  onChange={(e) => setEveryN(e.target.value)}
                  aria-label="Pages per file"
                  className="w-24 rounded border border-neutral-300 bg-white px-2 py-1.5 text-sm outline-none focus:border-neutral-500 dark:border-neutral-700 dark:bg-neutral-800"
                />
              </label>
            ) : null}

            {kind === "atPages" ? (
              <input
                value={atPages}
                onChange={(e) => setAtPages(e.target.value)}
                placeholder="Split before pages, e.g. 4, 7, 10"
                autoFocus
                aria-label="Split before pages"
                className="rounded border border-neutral-300 bg-white px-2 py-1.5 text-sm outline-none focus:border-neutral-500 dark:border-neutral-700 dark:bg-neutral-800"
              />
            ) : null}

            {kind === "bySize" ? (
              <label className="flex items-center gap-2 text-sm">
                <span className="text-neutral-600 dark:text-neutral-400">Target size (MB)</span>
                <input
                  type="number"
                  min={0}
                  step="0.1"
                  value={sizeMb}
                  onChange={(e) => setSizeMb(e.target.value)}
                  aria-label="Target size in megabytes"
                  className="w-24 rounded border border-neutral-300 bg-white px-2 py-1.5 text-sm outline-none focus:border-neutral-500 dark:border-neutral-700 dark:bg-neutral-800"
                />
              </label>
            ) : null}

            {kind === "bookmarks" ? (
              <p className="text-sm text-neutral-600 dark:text-neutral-400">
                One file per top-level bookmark. Needs at least two top-level
                bookmarks.
              </p>
            ) : null}

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
                disabled={!mode}
                className="rounded bg-neutral-900 px-3 py-1.5 text-sm text-white hover:bg-neutral-800 disabled:opacity-50 dark:bg-neutral-100 dark:text-neutral-900 dark:hover:bg-neutral-200"
              >
                Choose folder…
              </button>
            </div>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
