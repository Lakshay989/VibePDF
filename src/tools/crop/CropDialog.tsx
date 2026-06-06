// SPEC: P2-PAGE-009 — crop dialog. The user trims margins (points) from
// each edge of the page; we convert to a PDF CropBox (origin bottom-left).
// A contained modal rather than a drag-select overlay on the viewer
// (deferred — see BACKLOG). Controlled component: the parent owns `target`.

import * as Dialog from "@radix-ui/react-dialog";
import { useEffect, useState } from "react";

import type { CropRect } from "@/ipc/crop";

export interface CropTarget {
  /** 0-based page index. */
  page: number;
  /** Origin of the page's current box (PDF.js `page.view` [x0,y0,…]),
   *  so margins map to absolute page-space coordinates even if already
   *  cropped. */
  x0: number;
  y0: number;
  /** Current box dimensions in points. */
  width: number;
  height: number;
}

export interface CropDialogProps {
  target: CropTarget | null;
  onApply: (rect: CropRect) => void;
  onReset: () => void;
  onClose: () => void;
}

type Edge = "top" | "bottom" | "left" | "right";

export function CropDialog({ target, onApply, onReset, onClose }: CropDialogProps) {
  const [margins, setMargins] = useState({ top: 0, bottom: 0, left: 0, right: 0 });

  // Reset the inputs whenever a new page is targeted.
  useEffect(() => {
    if (target) setMargins({ top: 0, bottom: 0, left: 0, right: 0 });
  }, [target]);

  const open = target !== null;
  const w = target?.width ?? 0;
  const h = target?.height ?? 0;
  const x0 = target?.x0 ?? 0;
  const y0 = target?.y0 ?? 0;
  // Margins (from each edge) → CropBox in absolute PDF points (y up from
  // bottom), offset by the page's current box origin.
  const rect: CropRect = {
    left: x0 + margins.left,
    bottom: y0 + margins.bottom,
    right: x0 + w - margins.right,
    top: y0 + h - margins.top,
  };
  const valid = open && rect.left < rect.right && rect.bottom < rect.top;

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
            Crop page {target ? target.page + 1 : ""}
          </Dialog.Title>
          <Dialog.Description className="mt-1 text-sm text-neutral-600 dark:text-neutral-400">
            Trim from each edge, in points. Page is {Math.round(w)} ×{" "}
            {Math.round(h)} pt.
          </Dialog.Description>

          <div className="mt-4 grid grid-cols-2 gap-3">
            {(["top", "bottom", "left", "right"] as Edge[]).map((edge) => (
              <label key={edge} className="flex flex-col gap-1 text-sm capitalize">
                {edge}
                <input
                  type="number"
                  min={0}
                  value={margins[edge]}
                  onChange={(e) =>
                    setMargins((m) => ({
                      ...m,
                      [edge]: Math.max(0, Number(e.target.value) || 0),
                    }))
                  }
                  className="rounded border border-neutral-300 bg-white px-2 py-1 text-sm outline-none focus:border-neutral-500 dark:border-neutral-700 dark:bg-neutral-800"
                />
              </label>
            ))}
          </div>

          {open && !valid ? (
            <div role="alert" className="mt-2 text-sm text-red-600 dark:text-red-400">
              Margins exceed the page size.
            </div>
          ) : null}

          <div className="mt-5 flex items-center justify-between">
            <button
              type="button"
              onClick={onReset}
              className="rounded border border-neutral-300 px-3 py-1.5 text-sm hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
            >
              Reset crop
            </button>
            <div className="flex gap-2">
              <button
                type="button"
                onClick={onClose}
                className="rounded border border-neutral-300 px-3 py-1.5 text-sm hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
              >
                Cancel
              </button>
              <button
                type="button"
                disabled={!valid}
                onClick={() => onApply(rect)}
                className="rounded bg-neutral-900 px-3 py-1.5 text-sm text-white hover:bg-neutral-800 disabled:opacity-50 dark:bg-neutral-100 dark:text-neutral-900 dark:hover:bg-neutral-200"
              >
                Apply
              </button>
            </div>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
