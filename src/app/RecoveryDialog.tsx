// SPEC: P2.A2 — startup prompt offering to recover unsaved changes left
// by a previous run that didn't exit cleanly. Controlled component: the
// parent (use-recovery) owns the entry list; this only renders it. The
// dialog is open whenever there's at least one entry, and closes once the
// user has recovered or discarded them all.

import * as Dialog from "@radix-ui/react-dialog";

import { basename } from "@/app/paths";
import type { RecoveryEntry } from "@/ipc/recovery";

export interface RecoveryDialogProps {
  entries: RecoveryEntry[];
  onRecover: (entry: RecoveryEntry) => void;
  onDiscard: (entry: RecoveryEntry) => void;
}

export function RecoveryDialog({
  entries,
  onRecover,
  onDiscard,
}: RecoveryDialogProps) {
  const open = entries.length > 0;

  return (
    <Dialog.Root open={open}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-black/40" />
        <Dialog.Content className="fixed left-1/2 top-1/2 w-[460px] max-w-[92%] -translate-x-1/2 -translate-y-1/2 rounded-lg bg-white p-5 shadow-xl dark:bg-neutral-900 dark:text-neutral-100">
          <Dialog.Title className="text-base font-semibold">
            Recover unsaved changes?
          </Dialog.Title>
          <Dialog.Description className="mt-1 text-sm text-neutral-600 dark:text-neutral-400">
            VibePDF found{" "}
            {entries.length === 1
              ? "a document"
              : `${entries.length} documents`}{" "}
            with unsaved changes from a previous session.
          </Dialog.Description>

          <ul className="mt-4 flex flex-col gap-2">
            {entries.map((entry) => (
              <li
                key={entry.documentId}
                className="flex items-center gap-2 rounded border border-neutral-200 px-3 py-2 dark:border-neutral-800"
              >
                <span
                  className="min-w-0 flex-1 truncate text-sm"
                  title={entry.originalPath}
                >
                  {basename(entry.originalPath)}
                </span>
                <button
                  type="button"
                  onClick={() => onRecover(entry)}
                  className="rounded bg-neutral-900 px-2 py-1 text-xs text-white hover:bg-neutral-800 dark:bg-neutral-100 dark:text-neutral-900 dark:hover:bg-neutral-200"
                >
                  Recover
                </button>
                <button
                  type="button"
                  onClick={() => onDiscard(entry)}
                  className="rounded border border-neutral-300 px-2 py-1 text-xs hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
                >
                  Discard
                </button>
              </li>
            ))}
          </ul>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
