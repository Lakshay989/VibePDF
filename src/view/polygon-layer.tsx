// SPEC: P3-ANN-004 (P3.C1b₂) — the per-page polygon overlay.
//
// Polygons don't fit the drag lifecycle (down→move→up→commit); they're a
// MULTI-CLICK gesture: click to drop each vertex, a rubber-band edge follows the
// cursor, double-click / Enter finishes, Esc cancels. So — like NoteLayer and
// FreeTextLayer — this is a self-contained overlay that owns its own gesture,
// rather than going through `stepTool`. On finish it persists via the actor (a
// /Polygon with a generated /AP) and the canvas renders it; the overlay only
// draws the in-progress shape.

import { type PointerEvent as ReactPointerEvent, useEffect, useState } from "react";

import { addPolygon } from "@/ipc/polygons";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useHistoryStore } from "@/state/history-store";
import { useToolStore } from "@/state/tool-store";
import { type PageGeometry, pdfToScreen, type ScreenPoint, screenToPdf } from "@/tools/_framework";

/** A click within this many CSS px of the last vertex is a duplicate (e.g. the
 *  second pointerdown of a double-click) and is ignored. */
const DEDUP_PX = 6;

export interface PolygonLayerProps {
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

export function PolygonLayer({
  documentId,
  page,
  displayedWidth,
  displayedHeight,
  scale,
  rotation,
}: PolygonLayerProps) {
  const activeTool = useToolStore((s) => s.activeTool);
  const options = useToolStore((s) => s.options);
  const setActiveTool = useToolStore((s) => s.setActiveTool);
  const setHistory = useHistoryStore((s) => s.setHistory);
  const bumpEpoch = useEditEpochStore((s) => s.bumpEpoch);

  // Vertices in PDF points (stable under scroll/zoom); cursor in layer px.
  const [vertices, setVertices] = useState<{ x: number; y: number }[]>([]);
  const [cursor, setCursor] = useState<ScreenPoint | null>(null);

  const drawing = activeTool === "polygon";

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
    if (pts.length < 3) {
      setActiveTool(null);
      return;
    }
    setActiveTool(null);
    addPolygon(
      documentId,
      page,
      true,
      pts.map((p) => [p.x, p.y]),
      options.color,
      options.fillColor,
      options.opacity,
      options.strokeWidth,
    )
      .then((h) => {
        bumpEpoch(documentId);
        setHistory(documentId, h);
      })
      .catch((err: unknown) => console.warn("add polygon failed", documentId, err));
  };

  // Enter finishes the polygon; Escape cancels — but only while one is in progress.
  useEffect(() => {
    if (!drawing || vertices.length === 0) return undefined;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Enter") {
        e.preventDefault();
        finish(vertices);
      } else if (e.key === "Escape") {
        e.preventDefault();
        reset();
        setActiveTool(null);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
    // `finish`/`reset` close over the current vertices; re-bind when they change.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [drawing, vertices]);

  const layerPoint = (e: ReactPointerEvent<Element>): ScreenPoint => {
    const r = e.currentTarget.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  };

  const onPointerDown = (e: ReactPointerEvent<Element>) => {
    if (!drawing) return;
    const screen = layerPoint(e);
    // Skip a click that lands on the previous vertex (the 2nd down of a dbl-click).
    const last = vertices[vertices.length - 1];
    if (last) {
      const ls = pdfToScreen({ page, x: last.x, y: last.y }, geo);
      if (Math.hypot(ls.x - screen.x, ls.y - screen.y) < DEDUP_PX) return;
    }
    const pdf = screenToPdf(screen, geo);
    setVertices((v) => [...v, { x: pdf.x, y: pdf.y }]);
  };

  const onPointerMove = (e: ReactPointerEvent<Element>) => {
    if (!drawing || vertices.length === 0) return;
    setCursor(layerPoint(e));
  };

  const onDoubleClick = () => {
    if (drawing) finish(vertices);
  };

  const screenPts = vertices.map((v) => pdfToScreen({ page, x: v.x, y: v.y }, geo));
  const solid = screenPts.map((p) => `${p.x},${p.y}`).join(" ");
  const v0 = screenPts[0];
  const lastPt = screenPts[screenPts.length - 1];

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
            <polyline points={solid} fill="none" stroke={options.color} strokeWidth={Math.max(1, options.strokeWidth * scale)} />
          ) : null}
          {/* Rubber-band: last vertex → cursor, and cursor → first (the closing edge). */}
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
          {cursor && v0 && screenPts.length > 1 ? (
            <line x1={cursor.x} y1={cursor.y} x2={v0.x} y2={v0.y} stroke={options.color} strokeWidth={1} strokeDasharray="2 3" />
          ) : null}
          {screenPts.map((p, i) => (
            <circle key={i} cx={p.x} cy={p.y} r={3} fill="#fff" stroke={options.color} strokeWidth={1} />
          ))}
        </>
      ) : null}
    </svg>
  );
}
