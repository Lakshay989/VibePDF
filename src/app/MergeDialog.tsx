// SPEC: P2-PAGE-008 (partial) — pick an ordered set of PDFs to merge into a
// new file. The list is seeded with the current document; "Add files…" appends
// more via a native picker; up/down set the merge order. On confirm the parent
// opens the save dialog and calls the backend. Controlled: the parent owns
// `open`. (Concatenation + annotations now; bookmarks/form-fields deferred.)

import * as Dialog from "@radix-ui/react-dialog";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";

import { moveDown, moveUp, removeAt } from "@/tools/merge/reorder";
import { basename } from "@/app/paths";

export interface MergeDialogProps {
  open: boolean;
  /** Seed list (e.g. the current document's path). */
  initialPaths: string[];
  /** Called with the ordered paths when the user confirms. */
  onMerge: (paths: string[]) => void;
  onClose: () => void;
}

export function MergeDialog({ open, initialPaths, onMerge, onClose }: MergeDialogProps) {
  const [paths, setPaths] = useState<string[]>(initialPaths);

  useEffect(() => {
    if (open) setPaths(initialPaths);
  }, [open, initialPaths]);

  const addFiles = async () => {
    const selected = await openFileDialog({
      multiple: true,
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
    if (selected === null) return; // cancelled
    const added = Array.isArray(selected) ? selected : [selected];
    setPaths((prev) => [...prev, ...added]);
  };

  const canMerge = paths.length >= 2;

  return (
    <Dialog.Root
      open={open}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-black/40" />
        <Dialog.Content className="fixed left-1/2 top-1/2 w-[460px] max-w-[92%] -translate-x-1/2 -translate-y-1/2 rounded-lg bg-white p-5 shadow-xl dark:bg-neutral-900 dark:text-neutral-100">
          <Dialog.Title className="text-base font-semibold">Merge PDFs</Dialog.Title>
          <Dialog.Description className="mt-1 text-sm text-neutral-600 dark:text-neutral-400">
            Combine files into one new PDF, in the order shown. Bookmarks and
            form fields are not carried yet.
          </Dialog.Description>

          <form
            className="mt-4 flex flex-col gap-3"
            onSubmit={(e) => {
              e.preventDefault();
              if (canMerge) onMerge(paths);
            }}
          >
            <ul className="max-h-64 divide-y divide-neutral-200 overflow-y-auto rounded border border-neutral-200 dark:divide-neutral-800 dark:border-neutral-800">
              {paths.length === 0 ? (
                <li className="px-3 py-2 text-sm text-neutral-500">No files yet.</li>
              ) : (
                paths.map((p, i) => (
                  <li key={`${p}:${i}`} className="flex items-center gap-2 px-2 py-1.5 text-sm">
                    <span className="w-5 text-right tabular-nums text-neutral-400">{i + 1}</span>
                    <span className="flex-1 truncate" title={p}>
                      {basename(p)}
                    </span>
                    <button
                      type="button"
                      onClick={() => setPaths((prev) => moveUp(prev, i))}
                      disabled={i === 0}
                      aria-label={`Move ${basename(p)} up`}
                      className="rounded px-1.5 py-0.5 hover:bg-neutral-100 disabled:opacity-30 dark:hover:bg-neutral-800"
                    >
                      ↑
                    </button>
                    <button
                      type="button"
                      onClick={() => setPaths((prev) => moveDown(prev, i))}
                      disabled={i === paths.length - 1}
                      aria-label={`Move ${basename(p)} down`}
                      className="rounded px-1.5 py-0.5 hover:bg-neutral-100 disabled:opacity-30 dark:hover:bg-neutral-800"
                    >
                      ↓
                    </button>
                    <button
                      type="button"
                      onClick={() => setPaths((prev) => removeAt(prev, i))}
                      aria-label={`Remove ${basename(p)}`}
                      className="rounded px-1.5 py-0.5 text-red-600 hover:bg-red-50 dark:text-red-400 dark:hover:bg-red-950"
                    >
                      ✕
                    </button>
                  </li>
                ))
              )}
            </ul>

            <div className="flex items-center justify-between">
              <button
                type="button"
                onClick={() => void addFiles()}
                className="rounded border border-neutral-300 px-3 py-1.5 text-sm hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
              >
                Add files…
              </button>
              <span className="text-xs text-neutral-500">
                {paths.length} file{paths.length === 1 ? "" : "s"}
                {canMerge ? "" : " — need at least 2"}
              </span>
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
                disabled={!canMerge}
                className="rounded bg-neutral-900 px-3 py-1.5 text-sm text-white hover:bg-neutral-800 disabled:opacity-50 dark:bg-neutral-100 dark:text-neutral-900 dark:hover:bg-neutral-200"
              >
                Merge…
              </button>
            </div>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
