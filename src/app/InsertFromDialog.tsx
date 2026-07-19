// SPEC: P2-PAGE-005 — insert pages from another PDF into the open document.
// Pick a source file (we peek its page count), choose which pages (default
// all), and where to insert. On confirm the parent calls the backend and
// bumps the edit epoch. Controlled: the parent owns `open`.
// (Content + annotations + dimensions now; form fields deferred to lopdf.)

import * as Dialog from "@radix-ui/react-dialog";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";

import { peekPageCount } from "@/ipc/peek";
import { parsePageRange } from "@/tools/extract/page-range";
import { basename } from "@/app/paths";

type Position = "start" | "end" | "after";

export interface InsertFromDialogProps {
  open: boolean;
  /** Page count of the open document (for the position selector). */
  destPageCount: number;
  /** Called with the source path, 0-based source pages, and 0-based insert index. */
  onInsert: (args: { sourcePath: string; pages: number[]; index: number }) => void;
  onClose: () => void;
}

export function InsertFromDialog({
  open,
  destPageCount,
  onInsert,
  onClose,
}: InsertFromDialogProps) {
  const [sourcePath, setSourcePath] = useState<string | null>(null);
  const [sourceCount, setSourceCount] = useState<number | null>(null);
  const [peekError, setPeekError] = useState<string | null>(null);
  const [rangeInput, setRangeInput] = useState("");
  const [position, setPosition] = useState<Position>("end");
  const [afterPage, setAfterPage] = useState(1);

  useEffect(() => {
    if (open) {
      setSourcePath(null);
      setSourceCount(null);
      setPeekError(null);
      setRangeInput("");
      setPosition("end");
      setAfterPage(1);
    }
  }, [open]);

  const chooseFile = async () => {
    const selected = await openFileDialog({
      multiple: false,
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
    if (typeof selected !== "string") return; // cancelled
    setSourcePath(selected);
    setSourceCount(null);
    setPeekError(null);
    try {
      setSourceCount(await peekPageCount(selected));
    } catch {
      setPeekError("Couldn't read that PDF.");
    }
  };

  // Resolve the chosen source pages (0-based). Empty input = all pages.
  let pages: number[] | null = null;
  let rangeError: string | null = null;
  if (sourceCount !== null) {
    if (rangeInput.trim().length === 0) {
      pages = Array.from({ length: sourceCount }, (_, i) => i);
    } else {
      const parsed = parsePageRange(rangeInput, sourceCount);
      if ("pages" in parsed) pages = parsed.pages;
      else rangeError = parsed.error;
    }
  }

  // Resolve the insert index (0-based) in the open document.
  const index =
    position === "start" ? 0 : position === "end" ? destPageCount : afterPage;

  const canInsert = sourcePath !== null && pages !== null && pages.length > 0;

  return (
    <Dialog.Root
      open={open}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-black/40" />
        <Dialog.Content className="fixed left-1/2 top-1/2 w-[440px] max-w-[92%] -translate-x-1/2 -translate-y-1/2 rounded-lg bg-white p-5 shadow-xl dark:bg-neutral-900 dark:text-neutral-100">
          <Dialog.Title className="text-base font-semibold">
            Insert pages from PDF
          </Dialog.Title>
          <Dialog.Description className="mt-1 text-sm text-neutral-600 dark:text-neutral-400">
            Copy pages from another file into this document. Form fields are not
            carried yet.
          </Dialog.Description>

          <form
            className="mt-4 flex flex-col gap-3"
            onSubmit={(e) => {
              e.preventDefault();
              if (canInsert && sourcePath && pages) {
                onInsert({ sourcePath, pages, index });
              }
            }}
          >
            <div className="flex items-center gap-2">
              <button
                type="button"
                onClick={() => void chooseFile()}
                className="rounded border border-neutral-300 px-3 py-1.5 text-sm hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
              >
                Choose file…
              </button>
              <span className="flex-1 truncate text-sm text-neutral-600 dark:text-neutral-400">
                {sourcePath ? (
                  <span title={sourcePath}>
                    {basename(sourcePath)}
                    {sourceCount !== null ? ` — ${sourceCount} page${sourceCount === 1 ? "" : "s"}` : ""}
                  </span>
                ) : (
                  "No file chosen"
                )}
              </span>
            </div>
            {peekError ? (
              <div role="alert" className="text-sm text-red-600 dark:text-red-400">
                {peekError}
              </div>
            ) : null}

            <label className="flex flex-col gap-1 text-sm">
              <span className="text-neutral-600 dark:text-neutral-400">
                Pages (blank = all)
              </span>
              <input
                value={rangeInput}
                onChange={(e) => setRangeInput(e.target.value)}
                placeholder="e.g. 1-3, 5"
                aria-label="Source pages"
                disabled={sourceCount === null}
                className="rounded border border-neutral-300 bg-white px-2 py-1.5 outline-none focus:border-neutral-500 disabled:opacity-50 dark:border-neutral-700 dark:bg-neutral-800"
              />
            </label>
            {rangeError ? (
              <div role="alert" className="text-sm text-red-600 dark:text-red-400">
                {rangeError}
              </div>
            ) : null}

            <div className="flex items-center gap-2 text-sm">
              <span className="text-neutral-600 dark:text-neutral-400">Insert</span>
              <select
                value={position}
                onChange={(e) => setPosition(e.target.value as Position)}
                aria-label="Insert position"
                className="rounded border border-neutral-300 bg-transparent px-1 py-1 dark:border-neutral-700"
              >
                <option value="start">at start</option>
                <option value="end">at end</option>
                <option value="after">after page…</option>
              </select>
              {position === "after" ? (
                <input
                  type="number"
                  min={1}
                  max={destPageCount}
                  value={afterPage}
                  onChange={(e) => {
                    const n = Number(e.target.value);
                    setAfterPage(Math.min(Math.max(1, n), destPageCount));
                  }}
                  aria-label="After page number"
                  className="w-20 rounded border border-neutral-300 bg-white px-2 py-1 outline-none focus:border-neutral-500 dark:border-neutral-700 dark:bg-neutral-800"
                />
              ) : null}
            </div>

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
                disabled={!canInsert}
                className="rounded bg-neutral-900 px-3 py-1.5 text-sm text-white hover:bg-neutral-800 disabled:opacity-50 dark:bg-neutral-100 dark:text-neutral-900 dark:hover:bg-neutral-200"
              >
                Insert
              </button>
            </div>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
