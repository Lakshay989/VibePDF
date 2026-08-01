// SPEC: P3-ANN-005 (P3.C2) — the per-page freehand ink overlay.
//
// Ink is a DRAG gesture (down → move-accumulate → up), but unlike the rect/line
// drag tools — which only need a start and an end — it captures the whole path,
// sample by sample, plus per-sample pressure. That doesn't fit `stepTool`'s
// two-point lifecycle, so — like NoteLayer / PolygonLayer — this is a
// self-contained overlay that owns its own gesture. On pointer-up it smooths the
// raw samples (Catmull-Rom) and persists a `/Ink` via the actor; the canvas then
// renders the variable-width ribbon. While drawing, this layer only previews the
// raw path.

import { reportError } from "@/app/report-error";
import { type PointerEvent as ReactPointerEvent, useRef, useState } from "react";

import { addInk } from "@/ipc/ink";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useHistoryStore } from "@/state/history-store";
import { useOptimisticEditStore, usePendingEdits } from "@/state/optimistic-edit-store";
import { useToolStore } from "@/state/tool-store";
import { type InkPoint, smoothInk } from "@/tools/ink/ink";
import { type PageGeometry, pdfToScreen, screenToPdf } from "@/tools/_framework";

/** Optimistic-preview payload: a committed stroke, in PDF points, awaiting bake. */
interface InkHeld {
  points: InkPoint[];
  color: string;
  opacity: number;
  strokeWidth: number;
}

/** A new sample closer than this (CSS px) to the previous one is jitter — drop it. */
const CAPTURE_DEDUP_PX = 1.5;
/** A mouse reports `pressure` 0.5 while a button is down; treat ≤0 as neutral. */
const NEUTRAL_PRESSURE = 0.5;

export interface InkLayerProps {
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

export function InkLayer({
  documentId,
  page,
  displayedWidth,
  displayedHeight,
  scale,
  rotation,
}: InkLayerProps) {
  const activeTool = useToolStore((s) => s.activeTool);
  const options = useToolStore((s) => s.options);
  const setHistory = useHistoryStore((s) => s.setHistory);
  // Soft bump: the overlay shows the stroke and the sidebar updates, without
  // forcing a main-view reload — the overlay is the display until the next bake.
  const bumpEpoch = useEditEpochStore((s) => s.bumpEpochSoft);

  // Captured samples in PDF points (stable under scroll/zoom); `capturing` flips
  // on pointer-down so a move only records while the pen is down.
  const [stroke, setStroke] = useState<InkPoint[]>([]);
  const capturing = useRef(false);

  const drawing = activeTool === "ink";

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

  const layerPoint = (e: ReactPointerEvent<Element>) => {
    const r = e.currentTarget.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  };

  const pressureOf = (e: ReactPointerEvent<Element>) =>
    e.pressure > 0 ? e.pressure : NEUTRAL_PRESSURE;

  const onPointerDown = (e: ReactPointerEvent<Element>) => {
    if (!drawing || e.button !== 0) return;
    // Suppress the browser's native text/drag selection for this gesture, so a
    // stroke that crosses text (or leaves the page) draws instead of selecting.
    e.preventDefault();
    e.currentTarget.setPointerCapture(e.pointerId);
    capturing.current = true;
    const pdf = screenToPdf(layerPoint(e), geo);
    setStroke([{ x: pdf.x, y: pdf.y, pressure: pressureOf(e) }]);
  };

  const onPointerMove = (e: ReactPointerEvent<Element>) => {
    if (!drawing || !capturing.current) return;
    const screen = layerPoint(e);
    setStroke((cur) => {
      const last = cur[cur.length - 1];
      if (last) {
        const ls = pdfToScreen({ page, x: last.x, y: last.y }, geo);
        if (Math.hypot(ls.x - screen.x, ls.y - screen.y) < CAPTURE_DEDUP_PX) return cur;
      }
      const pdf = screenToPdf(screen, geo);
      return [...cur, { x: pdf.x, y: pdf.y, pressure: pressureOf(e) }];
    });
  };

  const finish = (e: ReactPointerEvent<Element>) => {
    if (!capturing.current) return;
    capturing.current = false;
    if (e.currentTarget.hasPointerCapture(e.pointerId)) {
      e.currentTarget.releasePointerCapture(e.pointerId);
    }
    const raw = stroke;
    setStroke([]);
    if (raw.length < 2) return; // a tap is not a stroke

    const smoothed = smoothInk(raw);
    const pts = smoothed.map((p): [number, number, number] => [p.x, p.y, p.pressure]);
    // Show the committed stroke immediately (the ~3 s backend apply + reload
    // would otherwise leave a blank gap on a large PDF); prune it when its bake
    // lands, or drop it if the write fails.
    const oe = useOptimisticEditStore.getState();
    const key = oe.add(documentId, page, "ink", {
      points: smoothed,
      color: options.color,
      opacity: options.opacity,
      strokeWidth: options.strokeWidth,
    } satisfies InkHeld);
    addInk(documentId, page, pts, options.color, options.opacity, options.strokeWidth)
      .then((h) => {
        bumpEpoch(documentId);
        oe.tie(documentId, key, (useEditEpochStore.getState().bakeByDoc[documentId] ?? 0) + 1);
        setHistory(documentId, h);
      })
      .catch((err: unknown) => {
        oe.remove(documentId, key);
        reportError("Couldn't add ink", err);
      });
  };

  const pending = usePendingEdits<InkHeld>(documentId, page, "ink");

  const screenPts = stroke.map((p) => pdfToScreen({ page, x: p.x, y: p.y }, geo));
  const preview = screenPts.map((p) => `${p.x},${p.y}`).join(" ");

  return (
    <svg
      className="absolute left-0 top-0"
      width={cssWidth}
      height={cssHeight}
      style={{
        pointerEvents: drawing ? "auto" : "none",
        cursor: drawing ? "crosshair" : undefined,
        touchAction: drawing ? "none" : undefined,
        userSelect: drawing ? "none" : undefined,
      }}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={finish}
      onPointerCancel={finish}
    >
      {pending.map(({ key, data }) => {
        const sp = data.points.map((p) => pdfToScreen({ page, x: p.x, y: p.y }, geo));
        if (sp.length < 2) return null;
        return (
          <polyline
            key={key}
            points={sp.map((p) => `${p.x},${p.y}`).join(" ")}
            fill="none"
            stroke={data.color}
            strokeWidth={Math.max(1, data.strokeWidth * scale)}
            strokeLinecap="round"
            strokeLinejoin="round"
            opacity={data.opacity}
          />
        );
      })}
      {screenPts.length > 1 ? (
        <polyline
          points={preview}
          fill="none"
          stroke={options.color}
          strokeWidth={Math.max(1, options.strokeWidth * scale)}
          strokeLinecap="round"
          strokeLinejoin="round"
          opacity={options.opacity}
        />
      ) : null}
    </svg>
  );
}
