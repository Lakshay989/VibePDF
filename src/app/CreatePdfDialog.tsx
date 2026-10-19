// SPEC: P7-OCR-009 (P7.C1a–c) — create a PDF from Word documents, HTML pages,
// images and text files.
//
// Pick files (in order), choose a page size, then a save dialog; the backend
// writes the PDF and the parent opens it. Self-contained like the compress
// dialog: it owns the save dialog, the call and the reporting, so the parent
// only has to open what was made. Works with no document open.

import * as Dialog from "@radix-ui/react-dialog";
import { open as openFileDialog, save as saveFileDialog } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";

import { basename } from "@/app/paths";
import { reportError } from "@/app/report-error";
import { type CreatePageSize, createPdfFromFiles } from "@/ipc/create";
import { useToastStore } from "@/state/toast-store";
import { moveDown, moveUp, removeAt } from "@/tools/merge/reorder";

/** What the native picker offers. */
export const CREATE_EXTENSIONS = [
  "docx",
  "html",
  "htm",
  "jpg",
  "jpeg",
  "png",
  "tif",
  "tiff",
  "webp",
  "txt",
  "text",
  "md",
  "log",
  "csv",
];

export interface CreatePdfDialogProps {
  open: boolean;
  onClose: () => void;
  /** Called with the new file's path once it is written. */
  onCreated: (path: string) => void;
}

const SIZES: { value: CreatePageSize; label: string }[] = [
  { value: "a4", label: "A4" },
  { value: "letter", label: "Letter" },
  { value: "image", label: "Match each image" },
];

/** `holiday.jpg` → `holiday.pdf`, the save dialog's suggestion. */
export function suggestedName(paths: string[]): string {
  const first = paths[0];
  if (!first) return "created.pdf";
  const stem = basename(first).replace(/\.[^.]+$/, "");
  return `${stem || "created"}.pdf`;
}

export function CreatePdfDialog({ open, onClose, onCreated }: CreatePdfDialogProps) {
  const [paths, setPaths] = useState<string[]>([]);
  const [pageSize, setPageSize] = useState<CreatePageSize>("a4");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (open) {
      setPaths([]);
      setBusy(false);
    }
  }, [open]);

  const addFiles = async () => {
    const selected = await openFileDialog({
      multiple: true,
      filters: [{ name: "Word documents, web pages, images and text", extensions: CREATE_EXTENSIONS }],
    });
    if (selected === null) return; // cancelled
    const added = Array.isArray(selected) ? selected : [selected];
    setPaths((prev) => [...prev, ...added]);
  };

  const create = async () => {
    const dest = await saveFileDialog({
      defaultPath: suggestedName(paths),
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
    if (typeof dest !== "string") return; // cancelled
    setBusy(true);
    try {
      const summary = await createPdfFromFiles(paths, dest, pageSize);
      for (const warning of summary.warnings) useToastStore.getState().push("info", warning);
      onClose();
      onCreated(dest);
    } catch (err) {
      reportError("Couldn't create the PDF", err);
    } finally {
      setBusy(false);
    }
  };

  const canCreate = paths.length > 0 && !busy;

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
          <Dialog.Title className="text-base font-semibold">Create PDF</Dialog.Title>
          <Dialog.Description className="mt-1 text-sm text-neutral-600 dark:text-neutral-400">
            Word documents, web pages (HTML), images (JPEG, PNG, TIFF, WebP) and plain-text files, one
            after another in the order shown. Word documents keep their own page size.
          </Dialog.Description>

          <form
            className="mt-4 flex flex-col gap-3"
            onSubmit={(e) => {
              e.preventDefault();
              if (canCreate) void create();
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

            <div className="flex items-center justify-between gap-3">
              <button
                type="button"
                onClick={() => void addFiles()}
                className="rounded border border-neutral-300 px-3 py-1.5 text-sm hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
              >
                Add files…
              </button>
              <label className="flex items-center gap-2 text-sm">
                Page size
                <select
                  value={pageSize}
                  onChange={(e) => {
                    const chosen = SIZES.find((s) => s.value === e.target.value);
                    if (chosen) setPageSize(chosen.value);
                  }}
                  className="rounded border border-neutral-300 bg-transparent px-2 py-1 dark:border-neutral-700"
                >
                  {SIZES.map((s) => (
                    <option key={s.value} value={s.value}>
                      {s.label}
                    </option>
                  ))}
                </select>
              </label>
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
                disabled={!canCreate}
                className="rounded bg-neutral-900 px-3 py-1.5 text-sm text-white hover:bg-neutral-800 disabled:opacity-50 dark:bg-neutral-100 dark:text-neutral-900 dark:hover:bg-neutral-200"
              >
                {busy ? "Creating…" : "Create…"}
              </button>
            </div>
          </form>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
