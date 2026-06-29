// SPEC: P4-EDIT-009 (P4.D2) — choose a text or image watermark and stamp it on
// selected pages, on top of or behind content, with opacity + rotation. The
// dialog calls the backend directly and bumps the edit epoch so the canvas
// re-renders. Controlled component: the parent owns `open`.

import * as Dialog from "@radix-ui/react-dialog";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { useEffect, useState } from "react";

import { addImageWatermark, addTextWatermark } from "@/ipc/watermark";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useHistoryStore } from "@/state/history-store";
import { DEFAULT_WATERMARK, parsePageRange } from "@/tools/watermark/watermark";

export interface WatermarkDialogProps {
  open: boolean;
  documentId: string;
  pageCount: number;
  onClose: () => void;
}

const FONTS = ["Helvetica", "Times", "Courier"] as const;

export function WatermarkDialog({ open, documentId, pageCount, onClose }: WatermarkDialogProps) {
  const setHistory = useHistoryStore((s) => s.setHistory);
  const bumpEpoch = useEditEpochStore((s) => s.bumpEpoch);

  const [kind, setKind] = useState<"text" | "image">("text");
  const [text, setText] = useState<string>(DEFAULT_WATERMARK.text);
  const [fontFamily, setFontFamily] = useState<(typeof FONTS)[number]>("Helvetica");
  const [fontSize, setFontSize] = useState(String(DEFAULT_WATERMARK.fontSize));
  const [color, setColor] = useState<string>(DEFAULT_WATERMARK.color);
  const [imagePath, setImagePath] = useState<string | null>(null);
  const [opacity, setOpacity] = useState(String(DEFAULT_WATERMARK.opacity));
  const [rotation, setRotation] = useState(String(DEFAULT_WATERMARK.rotation));
  const [range, setRange] = useState("all");
  const [behind, setBehind] = useState<boolean>(DEFAULT_WATERMARK.behind);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (open) {
      setKind("text");
      setText(DEFAULT_WATERMARK.text);
      setImagePath(null);
      setRange("all");
      setBehind(DEFAULT_WATERMARK.behind);
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

  const apply = () => {
    const parsed = parsePageRange(range, pageCount);
    if ("error" in parsed) {
      setError(parsed.error);
      return;
    }
    const op = Number(opacity);
    const rot = Number(rotation);
    if (!Number.isFinite(op) || op < 0 || op > 1) {
      setError("Opacity must be between 0 and 1.");
      return;
    }
    if (!Number.isFinite(rot)) {
      setError("Rotation must be a number.");
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
    if (kind === "text") {
      if (text.trim() === "") {
        setError("Enter watermark text.");
        setBusy(false);
        return;
      }
      const size = Number(fontSize);
      addTextWatermark(documentId, parsed.pages, text, fontFamily, size, color, op, rot, behind)
        .then(done)
        .catch(fail);
    } else {
      if (!imagePath) {
        setError("Choose an image.");
        setBusy(false);
        return;
      }
      addImageWatermark(documentId, parsed.pages, imagePath, op, rot, behind)
        .then(done)
        .catch(fail);
    }
  };

  const inputCls =
    "rounded border border-neutral-300 bg-white px-2 py-1.5 text-sm outline-none focus:border-neutral-500 dark:border-neutral-700 dark:bg-neutral-800";

  return (
    <Dialog.Root open={open} onOpenChange={(next) => !next && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 bg-black/40" />
        <Dialog.Content className="fixed left-1/2 top-1/2 w-[440px] max-w-[92%] -translate-x-1/2 -translate-y-1/2 rounded-lg bg-white p-5 shadow-xl dark:bg-neutral-900 dark:text-neutral-100">
          <Dialog.Title className="text-base font-semibold">Watermark</Dialog.Title>
          <Dialog.Description className="mt-1 text-sm text-neutral-600 dark:text-neutral-400">
            Stamp text or an image across pages. This document has {pageCount} page
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
                <input type="radio" checked={kind === "text"} onChange={() => setKind("text")} />
                Text
              </label>
              <label className="flex items-center gap-1.5">
                <input type="radio" checked={kind === "image"} onChange={() => setKind("image")} />
                Image
              </label>
            </div>

            {kind === "text" ? (
              <>
                <input
                  value={text}
                  onChange={(e) => setText(e.target.value)}
                  placeholder="Watermark text, e.g. DRAFT"
                  aria-label="Watermark text"
                  autoFocus
                  className={inputCls}
                />
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
                    className={`w-20 ${inputCls}`}
                  />
                  <input
                    type="color"
                    value={color}
                    onChange={(e) => setColor(e.target.value)}
                    aria-label="Watermark color"
                    className="h-8 w-10 rounded border border-neutral-300 dark:border-neutral-700"
                  />
                </div>
              </>
            ) : (
              <div className="flex items-center gap-2 text-sm">
                <button
                  type="button"
                  onClick={pickImage}
                  className="rounded border border-neutral-300 px-3 py-1.5 hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
                >
                  Choose image…
                </button>
                <span className="truncate text-neutral-600 dark:text-neutral-400">
                  {imagePath ? imagePath.split("/").pop() : "PNG or JPEG"}
                </span>
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
                <span className="text-neutral-600 dark:text-neutral-400">Rotation°</span>
                <input
                  type="number"
                  value={rotation}
                  onChange={(e) => setRotation(e.target.value)}
                  aria-label="Rotation"
                  className={`w-20 ${inputCls}`}
                />
              </label>
            </div>

            <label className="flex items-center gap-2 text-sm">
              <span className="text-neutral-600 dark:text-neutral-400">Pages</span>
              <input
                value={range}
                onChange={(e) => setRange(e.target.value)}
                placeholder="all, or 1-3, 5"
                aria-label="Pages"
                className={`flex-1 ${inputCls}`}
              />
            </label>

            <select
              value={behind ? "behind" : "ontop"}
              onChange={(e) => setBehind(e.target.value === "behind")}
              aria-label="Placement"
              className={inputCls}
            >
              <option value="behind">Behind content</option>
              <option value="ontop">On top of content</option>
            </select>

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
