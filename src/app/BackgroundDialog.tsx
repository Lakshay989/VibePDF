// SPEC: P4-EDIT-008 (P4.D1) — choose a colour or image background and fill it
// behind the content of selected pages. The dialog calls the backend directly
// and bumps the edit epoch so the canvas re-renders. Controlled component: the
// parent owns `open`. (A PDF-page source is deferred to D1b.)

import * as Dialog from "@radix-ui/react-dialog";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";

import { addColorBackground, addImageBackground, addPdfBackground } from "@/ipc/background";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useHistoryStore } from "@/state/history-store";
import { parsePageRange } from "@/tools/page-range";

export interface BackgroundDialogProps {
  open: boolean;
  documentId: string;
  pageCount: number;
  onClose: () => void;
}

const DEFAULT_COLOR = "#e6f0ff";

export function BackgroundDialog({ open, documentId, pageCount, onClose }: BackgroundDialogProps) {
  const setHistory = useHistoryStore((s) => s.setHistory);
  const bumpEpoch = useEditEpochStore((s) => s.bumpEpoch);

  const [kind, setKind] = useState<"color" | "image" | "pdf">("color");
  const [color, setColor] = useState(DEFAULT_COLOR);
  const [imagePath, setImagePath] = useState<string | null>(null);
  const [sourcePath, setSourcePath] = useState<string | null>(null);
  const [sourcePage, setSourcePage] = useState("1");
  const [opacity, setOpacity] = useState("1");
  const [range, setRange] = useState("all");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (open) {
      setKind("color");
      setColor(DEFAULT_COLOR);
      setImagePath(null);
      setSourcePath(null);
      setSourcePage("1");
      setOpacity("1");
      setRange("all");
      setError(null);
      setBusy(false);
    }
  }, [open]);

  const pickImage = () => {
    void openFileDialog({
      multiple: false,
      filters: [{ name: "Image", extensions: ["png", "jpg", "jpeg"] }],
    }).then((picked) => {
      if (typeof picked === "string") setImagePath(picked);
    });
  };

  const pickSource = () => {
    void openFileDialog({
      multiple: false,
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    }).then((picked) => {
      if (typeof picked === "string") setSourcePath(picked);
    });
  };

  const apply = () => {
    const parsed = parsePageRange(range, pageCount);
    if ("error" in parsed) {
      setError(parsed.error);
      return;
    }
    const op = Number(opacity);
    if (!Number.isFinite(op) || op < 0 || op > 1) {
      setError("Opacity must be between 0 and 1.");
      return;
    }

    const done = (h: Parameters<typeof setHistory>[1]) => {
      bumpEpoch(documentId);
      setHistory(documentId, h);
      onClose();
    };
    const fail = (err: unknown) => {
      setError(err instanceof Error ? err.message : String(err));
      setBusy(false);
    };

    setBusy(true);
    setError(null);
    if (kind === "color") {
      addColorBackground(documentId, parsed.pages, color, op).then(done).catch(fail);
    } else if (kind === "image") {
      if (!imagePath) {
        setError("Choose an image.");
        setBusy(false);
        return;
      }
      addImageBackground(documentId, parsed.pages, imagePath, op).then(done).catch(fail);
    } else {
      if (!sourcePath) {
        setError("Choose a source PDF.");
        setBusy(false);
        return;
      }
      const srcPage = Number(sourcePage);
      if (!Number.isInteger(srcPage) || srcPage < 1) {
        setError("Source page must be 1 or greater.");
        setBusy(false);
        return;
      }
      // The user types a 1-based page; the command takes a 0-based index.
      addPdfBackground(documentId, parsed.pages, sourcePath, srcPage - 1, op).then(done).catch(fail);
    }
  };

  const inputCls =
    "rounded border border-neutral-300 bg-white px-2 py-1.5 text-sm outline-none focus:border-neutral-500 dark:border-neutral-700 dark:bg-neutral-800";

  return (
    <Dialog.Root open={open} onOpenChange={(next) => !next && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-black/40" />
        <Dialog.Content className="fixed left-1/2 top-1/2 w-[440px] max-w-[92%] -translate-x-1/2 -translate-y-1/2 rounded-lg bg-white p-5 shadow-xl dark:bg-neutral-900 dark:text-neutral-100">
          <Dialog.Title className="text-base font-semibold">Background</Dialog.Title>
          <Dialog.Description className="mt-1 text-sm text-neutral-600 dark:text-neutral-400">
            Fill a colour or image behind page content. This document has {pageCount} page
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
                <input type="radio" checked={kind === "color"} onChange={() => setKind("color")} />
                Colour
              </label>
              <label className="flex items-center gap-1.5">
                <input type="radio" checked={kind === "image"} onChange={() => setKind("image")} />
                Image
              </label>
              <label className="flex items-center gap-1.5">
                <input type="radio" checked={kind === "pdf"} onChange={() => setKind("pdf")} />
                PDF page
              </label>
            </div>

            {kind === "color" ? (
              <label className="flex items-center gap-2 text-sm">
                <span className="text-neutral-600 dark:text-neutral-400">Colour</span>
                <input
                  type="color"
                  value={color}
                  onChange={(e) => setColor(e.target.value)}
                  aria-label="Background color"
                  className="h-8 w-12 rounded border border-neutral-300 dark:border-neutral-700"
                />
              </label>
            ) : kind === "image" ? (
              <div className="flex items-center gap-2 text-sm">
                <button
                  type="button"
                  onClick={pickImage}
                  className="rounded border border-neutral-300 px-3 py-1.5 hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
                >
                  Choose image…
                </button>
                <span className="truncate text-neutral-600 dark:text-neutral-400">
                  {imagePath ? imagePath.split("/").pop() : "PNG or JPEG, cover-fit"}
                </span>
              </div>
            ) : (
              <div className="flex items-center gap-2 text-sm">
                <button
                  type="button"
                  onClick={pickSource}
                  className="rounded border border-neutral-300 px-3 py-1.5 hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
                >
                  Choose PDF…
                </button>
                <span className="flex-1 truncate text-neutral-600 dark:text-neutral-400">
                  {sourcePath ? sourcePath.split("/").pop() : "source PDF"}
                </span>
                <label className="flex items-center gap-1">
                  <span className="text-neutral-600 dark:text-neutral-400">Page</span>
                  <input
                    type="number"
                    min={1}
                    value={sourcePage}
                    onChange={(e) => setSourcePage(e.target.value)}
                    aria-label="Source page"
                    className={`w-16 ${inputCls}`}
                  />
                </label>
              </div>
            )}

            <div className="flex items-center gap-3 text-sm">
              <label className="flex flex-1 items-center gap-2">
                <span className="text-neutral-600 dark:text-neutral-400">Opacity</span>
                <input
                  type="number"
                  min={0}
                  max={1}
                  step={0.05}
                  value={opacity}
                  onChange={(e) => setOpacity(e.target.value)}
                  aria-label="Opacity"
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
