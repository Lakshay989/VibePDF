// SPEC: P3-ANN-007 (P3.C4a) — the per-page measurement overlay.
//
// Reuses the polygon multi-click gesture for all three modes: distance is a
// 2-click segment (auto-finishes on the 2nd click), perimeter an open multi-click
// path, area a closed multi-click ring (double-click / Enter / click-the-first-
// dot to finish, Esc cancels). When the store is `calibrating`, the same 2-click
// captures a *reference* segment whose point-length is handed to the calibrate
// dialog instead of being persisted. On a real finish it computes the value
// against the doc's calibration and persists a `/Line`/`/PolyLine`/`/Polygon`
// (with a dimension `/IT`) via the actor. Self-contained like the other overlays.

import { reportError } from "@/app/report-error";
import { type PointerEvent as ReactPointerEvent, useEffect, useState } from "react";

import { addMeasure } from "@/ipc/measure";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useHistoryStore } from "@/state/history-store";
import { calibrationFor, useMeasureStore } from "@/state/measure-store";
import { useToolStore } from "@/state/tool-store";
import { type PageGeometry, pdfToScreen, type ScreenPoint, screenToPdf } from "@/tools/_framework";
import {
  formatMeasurement,
  measureValue,
  minPoints,
  straightDistance,
} from "@/tools/measure/measure";

/** A click within this many CSS px of the previous vertex is a duplicate. */
const DEDUP_PX = 6;
/** A click within this many CSS px of the first vertex closes an area ring. */
const CLOSE_PX = 12;

export interface MeasureLayerProps {
  documentId: string;
  page: number;
  displayedWidth: number;
  displayedHeight: number;
  scale: number;
  rotation: number;
}

export function MeasureLayer({
  documentId,
  page,
  displayedWidth,
  displayedHeight,
  scale,
  rotation,
}: MeasureLayerProps) {
  const activeTool = useToolStore((s) => s.activeTool);
  const options = useToolStore((s) => s.options);
  const kind = useMeasureStore((s) => s.kind);
  const calibrating = useMeasureStore((s) => s.calibrating);
  const setPendingRef = useMeasureStore((s) => s.setPendingRef);
  const cancelCalibrating = useMeasureStore((s) => s.cancelCalibrating);
  const calibration = useMeasureStore((s) => s.calibration);
  const setHistory = useHistoryStore((s) => s.setHistory);
  const bumpEpoch = useEditEpochStore((s) => s.bumpEpoch);

  const [vertices, setVertices] = useState<{ x: number; y: number }[]>([]);
  const [cursor, setCursor] = useState<ScreenPoint | null>(null);

  const drawing = activeTool === "measure";
  // While calibrating, the gesture is always a 2-point reference (distance).
  const effectiveKind = calibrating ? "distance" : kind;
  const closed = effectiveKind === "area";

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

  const reset = () => {
    setVertices([]);
    setCursor(null);
  };

  const finish = (pts: { x: number; y: number }[]) => {
    reset();
    if (calibrating) {
      if (pts.length >= 2) setPendingRef(straightDistance(pts));
      else cancelCalibrating();
      return;
    }
    if (pts.length < minPoints(effectiveKind)) return;
    const cal = calibrationFor(calibration, documentId);
    const value = measureValue(effectiveKind, pts, cal);
    const label = formatMeasurement(effectiveKind, value, cal.unit);
    addMeasure(
      documentId,
      page,
      effectiveKind,
      pts.map((p) => [p.x, p.y]),
      options.color,
      label,
      options.opacity,
      options.strokeWidth,
      cal.unitsPerPoint,
      cal.unit,
    )
      .then((h) => {
        bumpEpoch(documentId);
        setHistory(documentId, h);
      })
      .catch((err: unknown) => reportError("Couldn't add measurement", err));
  };

  // Leaving the tool abandons an in-progress measurement (no lingering rubber-band).
  useEffect(() => {
    if (!drawing) reset();
  }, [drawing]);

  // Enter finishes a multi-point measurement; Escape cancels — while one's going.
  useEffect(() => {
    if (!drawing || vertices.length === 0) return undefined;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Enter") {
        e.preventDefault();
        finish(vertices);
      } else if (e.key === "Escape") {
        e.preventDefault();
        reset();
        if (calibrating) cancelCalibrating();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [drawing, vertices, calibrating]);

  const layerPoint = (e: ReactPointerEvent<Element>): ScreenPoint => {
    const r = e.currentTarget.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  };

  const onPointerDown = (e: ReactPointerEvent<Element>) => {
    if (!drawing || e.button !== 0) return;
    const screen = layerPoint(e);
    // Click the first dot to close an area ring (≥3 points down).
    if (closed && vertices.length >= 3) {
      const v0 = vertices[0];
      const v0s = pdfToScreen({ page, x: v0.x, y: v0.y }, geo);
      if (Math.hypot(v0s.x - screen.x, v0s.y - screen.y) < CLOSE_PX) {
        finish(vertices);
        return;
      }
    }
    // Skip the 2nd down of a double-click (lands on the previous vertex).
    const last = vertices[vertices.length - 1];
    if (last) {
      const ls = pdfToScreen({ page, x: last.x, y: last.y }, geo);
      if (Math.hypot(ls.x - screen.x, ls.y - screen.y) < DEDUP_PX) return;
    }
    const pdf = screenToPdf(screen, geo);
    const next = [...vertices, { x: pdf.x, y: pdf.y }];
    setVertices(next);
    // Distance (and the calibration reference) finishes on the 2nd point.
    if (effectiveKind === "distance" && next.length >= 2) finish(next);
  };

  const onPointerMove = (e: ReactPointerEvent<Element>) => {
    if (!drawing || vertices.length === 0) return;
    setCursor(layerPoint(e));
  };

  const onDoubleClick = () => {
    if (drawing && effectiveKind !== "distance") finish(vertices);
  };

  const screenPts = vertices.map((v) => pdfToScreen({ page, x: v.x, y: v.y }, geo));
  const solid = screenPts.map((p) => `${p.x},${p.y}`).join(" ");
  const v0 = screenPts[0];
  const lastPt = screenPts[screenPts.length - 1];
  const strokePx = Math.max(1, options.strokeWidth * scale);

  return (
    <svg
      className="absolute left-0 top-0"
      width={cssWidth}
      height={cssHeight}
      style={{ pointerEvents: drawing ? "auto" : "none", cursor: drawing ? "crosshair" : undefined }}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onDoubleClick={onDoubleClick}
    >
      {screenPts.length > 0 ? (
        <>
          {screenPts.length > 1 ? (
            <polyline points={solid} fill="none" stroke={options.color} strokeWidth={strokePx} />
          ) : null}
          {cursor && lastPt ? (
            <line
              x1={lastPt.x}
              y1={lastPt.y}
              x2={cursor.x}
              y2={cursor.y}
              stroke={options.color}
              strokeWidth={1}
              strokeDasharray="4 4"
            />
          ) : null}
          {closed && cursor && v0 && screenPts.length > 1 ? (
            <line x1={cursor.x} y1={cursor.y} x2={v0.x} y2={v0.y} stroke={options.color} strokeWidth={1} strokeDasharray="2 3" />
          ) : null}
          {screenPts.map((p, i) => {
            const closable = closed && i === 0 && screenPts.length >= 3;
            return (
              <circle
                key={i}
                cx={p.x}
                cy={p.y}
                r={closable ? 6 : 3}
                fill={closable ? options.color : "#fff"}
                stroke={options.color}
                strokeWidth={closable ? 2 : 1}
              />
            );
          })}
        </>
      ) : null}
    </svg>
  );
}
