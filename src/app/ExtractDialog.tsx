// SPEC: P2-PAGE-006 — pick pages to extract into a new PDF. The user types
// a range ("1-3, 5"); on confirm the parent opens the native save dialog
// and calls the backend. Controlled component: the parent owns `open`.

import * as Dialog from "@radix-ui/react-dialog";
import { useEffect, useState } from "react";

import { parsePageRange } from "@/tools/extract/page-range";

export interface ExtractDialogProps {
  open: boolean;
  pageCount: number;
  /** Called with 0-based page indices when the user confirms. */
  onExtract: (pages: number[]) => void;
  onClose: () => void;
}

export function ExtractDialog({
  open,
  pageCount,
  onExtract,
  onClose,
}: ExtractDialogProps) {
  const [input, setInput] = useState("");
  useEffect(() => {
    if (open) setInput("");
  }, [open]);

  const parsed = parsePageRange(input, pageCount);
  const pages = "pages" in parsed ? parsed.pages : null;
  // Only surface an error once the user has typed something.
  const error = input.trim().length > 0 && "error" in parsed ? parsed.error : null;

  return (
    <Dialog.Root
      open={open}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-black/40" />
        <Dialog.Content className="fixed left-1/2 top-1/2 w-[380px] max-w-[92%] -translate-x-1/2 -translate-y-1/2 rounded-lg bg-white p-5 shadow-xl dark:bg-neutral-900 dark:text-neutral-100">
          <Dialog.Title className="text-base font-semibold">
            Extract pages
          </Dialog.Title>
          <Dialog.Description className="mt-1 text-sm text-neutral-600 dark:text-neutral-400">
            Copy pages into a new PDF. This document has {pageCount} page
            {pageCount === 1 ? "" : "s"}.
          </Dialog.Description>

          <form
            className="mt-4 flex flex-col gap-3"
            onSubmit={(e) => {
              e.preventDefault();
              if (pages && pages.length > 0) onExtract(pages);
            }}
          >
            <input
              value={input}
              onChange={(e) => setInput(e.target.value)}
              placeholder="e.g. 1-3, 5"
              autoFocus
              aria-label="Pages to extract"
              className="rounded border border-neutral-300 bg-white px-2 py-1.5 text-sm outline-none focus:border-neutral-500 dark:border-neutral-700 dark:bg-neutral-800"
            />
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
                disabled={!pages || pages.length === 0}
                className="rounded bg-neutral-900 px-3 py-1.5 text-sm text-white hover:bg-neutral-800 disabled:opacity-50 dark:bg-neutral-100 dark:text-neutral-900 dark:hover:bg-neutral-200"
              >
                Extract…
              </button>
            </div>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
