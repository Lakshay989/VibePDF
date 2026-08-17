// SPEC: P6-SEC-010 (P6.D1c) — the per-page "redact a region" overlay.
//
// Drag a box; on release a confirmation appears, because this is the one edit
// in VibePDF that is meant to be **irreversible**. Undo works until the file is
// saved and reopened, and after that the content is genuinely gone — which is
// the feature, not a limitation of it. The confirmation says so in those words
// rather than asking "Are you sure?", which tells a user nothing they did not
// already know.
//
// The drag mechanics follow `link-layer.tsx`: pointer events (not HTML5 DnD, per
// docs/04 on WKWebView), screen → PDF via the shared `_framework` helpers.

import { type PointerEvent as ReactPointerEvent, useState } from "react";

import { reportError } from "@/app/report-error";
import { redactRegion } from "@/ipc/pdf";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useHistoryStore } from "@/state/history-store";
import { useToolStore } from "@/state/tool-store";
import {
  normalizeScreenRect,
  type PageGeometry,
  type ScreenPoint,
  screenToPdf,
} from "@/tools/_framework";

export interface RedactLayerProps {
  documentId: string;
  /** 0-based page index. */
  page: number;
  /** Displayed (rotation-swapped) page size in PDF points. */
  displayedWidth: number;
  displayedHeight: number;
  /** CSS px per point. */
  scale: number;
  rotation: number;
}

interface Pending {
  rect: [number, number, number, number];
  box: { left: number; top: number; width: number; height: number };
}

export function RedactLayer({
  documentId,
  page,
  displayedWidth,
  displayedHeight,
  scale,
  rotation,
}: RedactLayerProps) {
  const activeTool = useToolStore((s) => s.activeTool);
  const setActiveTool = useToolStore((s) => s.setActiveTool);
  const setHistory = useHistoryStore((s) => s.setHistory);
  const bumpEpoch = useEditEpochStore((s) => s.bumpEpoch);

  const [start, setStart] = useState<ScreenPoint | null>(null);
  const [current, setCurrent] = useState<ScreenPoint | null>(null);
  const [pending, setPending] = useState<Pending | null>(null);
  const [removeMetadata, setRemoveMetadata] = useState(false);
  const [busy, setBusy] = useState(false);

  const active = activeTool === "redact";
  if (!active && !pending) return null;

  // `screenToPdf` wants the UNROTATED page dimensions; swap back for 90°/270°,
  // exactly as `link-layer` does.
  const swapped = (((rotation % 180) + 180) % 180) === 90;
  const geo: PageGeometry = {
    page,
    width: swapped ? displayedHeight : displayedWidth,
    height: swapped ? displayedWidth : displayedHeight,
    scale,
    rotation,
  };

  const layerPoint = (e: ReactPointerEvent<Element>): ScreenPoint => {
    const r = e.currentTarget.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  };

  const onPointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (!active || pending) return;
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
    const box = normalizeScreenRect(start, layerPoint(e));
    setStart(null);
    setCurrent(null);
    // A stray click is not a redaction. Unlike a link, a zero-size region has
    // no sensible default size — nothing would be removed and the confirmation
    // would be asking about nothing.
    if (box.width < 4 || box.height < 4) return;

    const a = screenToPdf({ x: box.left, y: box.top }, geo);
    const b = screenToPdf({ x: box.left + box.width, y: box.top + box.height }, geo);
    setPending({
      rect: [
        Math.min(a.x, b.x),
        Math.min(a.y, b.y),
        Math.max(a.x, b.x),
        Math.max(a.y, b.y),
      ],
      box,
    });
  };

  const cancel = () => {
    setPending(null);
    setRemoveMetadata(false);
  };

  const apply = () => {
    if (!pending) return;
    const { rect } = pending;
    setBusy(true);
    void redactRegion(documentId, page, rect, { removeMetadata })
      .then((report) => {
        cancel();
        setActiveTool(null);
        bumpEpoch(documentId);
        setHistory(documentId, report.history);
      })
      .catch((err: unknown) => {
        // A refusal is a real outcome here — a page whose text lives in a form
        // is refused on purpose — so the message matters more than usual.
        reportError("Couldn't redact that area", err);
      })
      .finally(() => setBusy(false));
  };

  const preview =
    start && current ? normalizeScreenRect(start, current) : pending ? pending.box : null;

  return (
    <div
      data-testid="redact-layer"
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      className="absolute inset-0 z-20"
      style={{ cursor: active && !pending ? "crosshair" : "default" }}
    >
      {preview ? (
        <div
          data-testid="redact-preview"
          className="pointer-events-none absolute border-2 border-red-500 bg-black/70"
          style={{
            left: preview.left,
            top: preview.top,
            width: preview.width,
            height: preview.height,
          }}
        />
      ) : null}

      {pending ? (
        <div
          role="dialog"
          aria-label="Confirm redaction"
          onPointerDown={(e) => e.stopPropagation()}
          className="absolute z-30 w-72 rounded border border-neutral-300 bg-white p-2 text-xs shadow-lg dark:border-neutral-700 dark:bg-neutral-900"
          style={{
            left: Math.min(pending.box.left, displayedWidth * scale - 300),
            top: pending.box.top + pending.box.height + 6,
          }}
        >
          <p className="mb-2 font-medium">Remove everything in this area?</p>
          <p className="mb-2 text-neutral-500">
            The text and images underneath are deleted, not covered. You can undo this
            until you save and reopen the file — after that it is gone for good.
          </p>

          <label className="mb-2 flex items-center gap-1.5">
            <input
              type="checkbox"
              aria-label="Also remove document metadata"
              checked={removeMetadata}
              onChange={(e) => setRemoveMetadata(e.target.checked)}
            />
            <span>Also remove document metadata</span>
          </label>

          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={apply}
              disabled={busy}
              className="rounded bg-red-600 px-2 py-1 text-white disabled:opacity-40"
            >
              {busy ? "Redacting…" : "Redact"}
            </button>
            <button
              type="button"
              onClick={cancel}
              className="ml-auto rounded border border-neutral-300 px-2 py-1 dark:border-neutral-700"
            >
              Cancel
            </button>
          </div>
        </div>
      ) : null}
    </div>
  );
}
