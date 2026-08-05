// SPEC: P5-FORM-006 (P5.B1) — the per-page "create text field" overlay.
//
// When the Create-text-field tool is active, drag a box on the page; on release a
// popover asks for the field's name / default / max-length / multi-line / required.
// Confirm creates an AcroForm text field via addTextField and re-reads the form
// summary so the Form-mode entry point updates. Pointer events (WKWebView; docs/04).

import { reportError } from "@/app/report-error";
import { type PointerEvent as ReactPointerEvent, useState } from "react";

import { addTextField, readFormSummary } from "@/ipc/forms";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useFormStore } from "@/state/form-store";
import { useHistoryStore } from "@/state/history-store";
import { useToolStore } from "@/state/tool-store";
import { type PageGeometry, type ScreenPoint, screenToPdf } from "@/tools/_framework";
import { normalizeScreenRect, withDefaultSize } from "@/tools/_framework";

export interface FormCreateLayerProps {
  documentId: string;
  /** 0-based page index. */
  page: number;
  /** Displayed (rotation-swapped) page size in PDF points. */
  displayedWidth: number;
  displayedHeight: number;
  /** CSS px per point. */
  scale: number;
  /** Page display rotation in degrees. */
  rotation: number;
}

interface Pending {
  rect: [number, number, number, number];
  box: { left: number; top: number; width: number; height: number };
}

export function FormCreateLayer({
  documentId,
  page,
  displayedWidth,
  displayedHeight,
  scale,
  rotation,
}: FormCreateLayerProps) {
  const activeTool = useToolStore((s) => s.activeTool);
  const setActiveTool = useToolStore((s) => s.setActiveTool);
  const setHistory = useHistoryStore((s) => s.setHistory);
  const bumpEpoch = useEditEpochStore((s) => s.bumpEpoch);
  const setDetected = useFormStore((s) => s.setDetected);

  const [start, setStart] = useState<ScreenPoint | null>(null);
  const [current, setCurrent] = useState<ScreenPoint | null>(null);
  const [pending, setPending] = useState<Pending | null>(null);
  const [name, setName] = useState("");
  const [defaultValue, setDefaultValue] = useState("");
  const [maxLen, setMaxLen] = useState("");
  const [multiline, setMultiline] = useState(false);
  const [required, setRequired] = useState(false);

  const placing = activeTool === "create-text-field";

  const swapped = (((rotation % 180) + 180) % 180) === 90;
  const geo: PageGeometry = {
    page,
    width: swapped ? displayedHeight : displayedWidth,
    height: swapped ? displayedWidth : displayedHeight,
    scale,
    rotation,
  };
  const cssWidth = displayedWidth * scale;
  const cssHeight = displayedHeight * scale;

  const layerPoint = (e: ReactPointerEvent<Element>): ScreenPoint => {
    const r = e.currentTarget.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  };

  const resetForm = () => {
    setName("");
    setDefaultValue("");
    setMaxLen("");
    setMultiline(false);
    setRequired(false);
  };

  const onPointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (!placing || pending) return;
    e.currentTarget.setPointerCapture?.(e.pointerId);
    const p = layerPoint(e);
    setStart(p);
    setCurrent(p);
  };
  const onPointerMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (!start) return;
    setCurrent(layerPoint(e));
  };
  const onPointerUp = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (!start) return;
    const box = withDefaultSize(normalizeScreenRect(start, layerPoint(e)));
    setStart(null);
    setCurrent(null);
    const a = screenToPdf({ x: box.left, y: box.top }, geo);
    const b = screenToPdf({ x: box.left + box.width, y: box.top + box.height }, geo);
    const rect: [number, number, number, number] = [
      Math.min(a.x, b.x),
      Math.min(a.y, b.y),
      Math.max(a.x, b.x),
      Math.max(a.y, b.y),
    ];
    resetForm();
    setPending({ rect, box });
  };

  const cancel = () => {
    setPending(null);
    resetForm();
  };

  const confirm = () => {
    if (!pending || name.trim() === "") return;
    const { rect } = pending;
    const parsed = Number.parseInt(maxLen, 10);
    const field = {
      name: name.trim(),
      defaultValue,
      maxLen: maxLen.trim() !== "" && Number.isFinite(parsed) && parsed > 0 ? parsed : null,
      multiline,
      required,
    };
    cancel();
    setActiveTool(null);
    addTextField(documentId, page, rect, field)
      .then((h) => {
        bumpEpoch(documentId);
        setHistory(documentId, h);
        return readFormSummary(documentId).then(setDetected);
      })
      .catch((err: unknown) => reportError("Couldn't create the field", err));
  };

  if (!placing) return null;

  const preview = start && current ? normalizeScreenRect(start, current) : null;

  return (
    <div
      className="absolute left-0 top-0"
      style={{
        width: cssWidth,
        height: cssHeight,
        pointerEvents: "auto",
        cursor: pending ? "default" : "crosshair",
      }}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
    >
      {preview ? (
        <div
          className="absolute border-2 border-dashed border-blue-500 bg-blue-400/10"
          style={{ left: preview.left, top: preview.top, width: preview.width, height: preview.height }}
        />
      ) : null}

      {pending ? (
        <>
          <div
            className="absolute border-2 border-blue-500 bg-blue-400/10"
            style={{
              left: pending.box.left,
              top: pending.box.top,
              width: pending.box.width,
              height: pending.box.height,
            }}
          />
          <div
            className="absolute z-10 flex w-64 flex-col gap-2 rounded border border-neutral-300 bg-white p-3 text-sm shadow-lg"
            style={{
              left: Math.min(pending.box.left, cssWidth - 256),
              top: pending.box.top + pending.box.height + 6,
            }}
            onPointerDown={(e) => e.stopPropagation()}
          >
            <label className="flex flex-col gap-0.5">
              <span className="text-xs text-neutral-500">Field name</span>
              <input
                autoFocus
                aria-label="Field name"
                className="rounded border border-neutral-300 px-2 py-1"
                placeholder="e.g. email"
                value={name}
                onChange={(e) => setName(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") confirm();
                  if (e.key === "Escape") cancel();
                }}
              />
            </label>
            <label className="flex flex-col gap-0.5">
              <span className="text-xs text-neutral-500">Default value</span>
              <input
                aria-label="Default value"
                className="rounded border border-neutral-300 px-2 py-1"
                value={defaultValue}
                onChange={(e) => setDefaultValue(e.target.value)}
              />
            </label>
            <label className="flex flex-col gap-0.5">
              <span className="text-xs text-neutral-500">Max length (optional)</span>
              <input
                aria-label="Max length"
                type="number"
                min={1}
                className="rounded border border-neutral-300 px-2 py-1"
                value={maxLen}
                onChange={(e) => setMaxLen(e.target.value)}
              />
            </label>
            <label className="flex items-center gap-2">
              <input type="checkbox" checked={multiline} onChange={(e) => setMultiline(e.target.checked)} />
              <span>Multi-line</span>
            </label>
            <label className="flex items-center gap-2">
              <input type="checkbox" checked={required} onChange={(e) => setRequired(e.target.checked)} />
              <span>Required</span>
            </label>
            <div className="flex justify-end gap-2">
              <button
                type="button"
                className="rounded px-2 py-1 text-neutral-600 hover:bg-neutral-100"
                onClick={cancel}
              >
                Cancel
              </button>
              <button
                type="button"
                className="rounded bg-blue-600 px-2 py-1 text-white disabled:opacity-40"
                disabled={name.trim() === ""}
                onClick={confirm}
              >
                Create field
              </button>
            </div>
          </div>
        </>
      ) : null}
    </div>
  );
}
