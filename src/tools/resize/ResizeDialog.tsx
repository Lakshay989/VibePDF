// SPEC: P2-PAGE-010 — resize dialog. Pick a standard size (or custom W×H in
// points), choose preserve-aspect, and the scope (this page / all pages). A
// contained modal, mirroring CropDialog. Controlled component: the parent owns
// `target` and performs the resize.

import * as Dialog from "@radix-ui/react-dialog";
import { useEffect, useState } from "react";

import {
  findStandardSize,
  isValidDimension,
  matchStandardSize,
  STANDARD_SIZES,
} from "./page-sizes";

export interface ResizeTarget {
  /** 0-based index of the right-clicked page (the default single-page scope). */
  page: number;
  /** The page's current width/height in points, so the dialog opens showing
   *  the size you're changing *from*. */
  width: number;
  height: number;
}

export interface ResizeRequest {
  /** Target width in points. */
  width: number;
  /** Target height in points. */
  height: number;
  preserveAspect: boolean;
  /** Apply to just the targeted page, or every page in the document. */
  scope: "page" | "all";
}

export interface ResizeDialogProps {
  target: ResizeTarget | null;
  pageCount: number;
  onApply: (req: ResizeRequest) => void;
  onClose: () => void;
}

const CUSTOM = "Custom";

export function ResizeDialog({ target, pageCount, onApply, onClose }: ResizeDialogProps) {
  const [preset, setPreset] = useState("A4");
  const [customW, setCustomW] = useState(612);
  const [customH, setCustomH] = useState(792);
  const [preserveAspect, setPreserveAspect] = useState(true);
  const [scope, setScope] = useState<"page" | "all">("page");

  // Open showing the targeted page's *current* size: pre-select the matching
  // preset (or "Custom" with the current dimensions). Reset the rest.
  useEffect(() => {
    if (target) {
      const w = Math.round(target.width);
      const h = Math.round(target.height);
      setPreset(matchStandardSize(target.width, target.height) ?? CUSTOM);
      setCustomW(w);
      setCustomH(h);
      setPreserveAspect(true);
      setScope("page");
    }
  }, [target]);

  const open = target !== null;
  const std = findStandardSize(preset);
  const width = std ? std.width : customW;
  const height = std ? std.height : customH;
  const valid = open && isValidDimension(width) && isValidDimension(height);

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
            Resize page {target ? target.page + 1 : ""}
          </Dialog.Title>
          <Dialog.Description className="mt-1 text-sm text-neutral-600 dark:text-neutral-400">
            Currently {target ? Math.round(target.width) : 0} ×{" "}
            {target ? Math.round(target.height) : 0} pt. Scale the page content to
            a new size.
          </Dialog.Description>

          <div className="mt-4 flex flex-col gap-3">
            <label className="flex flex-col gap-1 text-sm">
              Size
              <select
                value={preset}
                onChange={(e) => setPreset(e.target.value)}
                className="rounded border border-neutral-300 bg-white px-2 py-1 text-sm outline-none focus:border-neutral-500 dark:border-neutral-700 dark:bg-neutral-800"
              >
                {STANDARD_SIZES.map((s) => (
                  <option key={s.name} value={s.name}>
                    {s.name} ({Math.round(s.width)} × {Math.round(s.height)} pt)
                  </option>
                ))}
                <option value={CUSTOM}>Custom…</option>
              </select>
            </label>

            {preset === CUSTOM ? (
              <div className="grid grid-cols-2 gap-3">
                <label className="flex flex-col gap-1 text-sm">
                  Width (pt)
                  <input
                    type="number"
                    min={1}
                    value={customW}
                    onChange={(e) => setCustomW(Number(e.target.value) || 0)}
                    className="rounded border border-neutral-300 bg-white px-2 py-1 text-sm outline-none focus:border-neutral-500 dark:border-neutral-700 dark:bg-neutral-800"
                  />
                </label>
                <label className="flex flex-col gap-1 text-sm">
                  Height (pt)
                  <input
                    type="number"
                    min={1}
                    value={customH}
                    onChange={(e) => setCustomH(Number(e.target.value) || 0)}
                    className="rounded border border-neutral-300 bg-white px-2 py-1 text-sm outline-none focus:border-neutral-500 dark:border-neutral-700 dark:bg-neutral-800"
                  />
                </label>
              </div>
            ) : null}

            <label className="flex items-center gap-2 text-sm">
              <input
                type="checkbox"
                checked={preserveAspect}
                onChange={(e) => setPreserveAspect(e.target.checked)}
              />
              Preserve aspect ratio (scale uniformly, centre content)
            </label>

            <fieldset className="flex flex-col gap-1 text-sm">
              <legend className="mb-1 text-neutral-600 dark:text-neutral-400">Apply to</legend>
              <label className="flex items-center gap-2">
                <input
                  type="radio"
                  name="resize-scope"
                  checked={scope === "page"}
                  onChange={() => setScope("page")}
                />
                This page
              </label>
              <label className="flex items-center gap-2">
                <input
                  type="radio"
                  name="resize-scope"
                  checked={scope === "all"}
                  onChange={() => setScope("all")}
                />
                All pages ({pageCount})
              </label>
            </fieldset>
          </div>

          {open && !valid ? (
            <div role="alert" className="mt-2 text-sm text-red-600 dark:text-red-400">
              Enter a positive width and height (max 14400 pt).
            </div>
          ) : null}

          <div className="mt-5 flex justify-end gap-2">
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
              onClick={() => onApply({ width, height, preserveAspect, scope })}
              className="rounded bg-neutral-900 px-3 py-1.5 text-sm text-white hover:bg-neutral-800 disabled:opacity-50 dark:bg-neutral-100 dark:text-neutral-900 dark:hover:bg-neutral-200"
            >
              Apply
            </button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
