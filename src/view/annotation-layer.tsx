// SPEC: infrastructure (P3.A2) — the per-page annotation overlay.
//
// An SVG layer absolutely positioned over each page canvas. It (1) draws the
// committed annotations + the live draft for its page (PDF → screen via the A1
// `coords` helpers), and (2) when a tool is active, captures pointer events and
// drives the A1 lifecycle, committing into the annotation store. When idle the
// overlay is click-through (`pointer-events: none`) except on the shapes
// themselves, so a click selects an annotation without blocking text selection
// or scrolling underneath.
//
// Pointer events, not HTML5 DnD (WKWebView; docs/04 §WebView quirks) — the
// drag uses `setPointerCapture`. No PDF bytes are touched here; persistence is
// P3.B1.

import { type PointerEvent as ReactPointerEvent, useEffect, useRef } from "react";

import { useAnnotationStore, useDocAnnotations } from "@/state/annotation-store";
import { useToolStore } from "@/state/tool-store";
import {
  getTool,
  IDLE,
  type MarkupAnnotation,
  type PageGeometry,
  type RectAnnotation,
  type PdfRect,
  pdfToScreen,
  type Quad,
  type ScreenPoint,
  screenToPdf,
  stepTool,
  type ToolSession,
} from "@/tools/_framework";

export interface AnnotationLayerProps {
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

export function AnnotationLayer({
  documentId,
  page,
  displayedWidth,
  displayedHeight,
  scale,
  rotation,
}: AnnotationLayerProps) {
  const annotations = useDocAnnotations(documentId);
  const draft = useAnnotationStore((s) => s.draft);
  const selectedId = useAnnotationStore((s) => s.selectedId);
  const setDraft = useAnnotationStore((s) => s.setDraft);
  const addAnnotation = useAnnotationStore((s) => s.add);
  const select = useAnnotationStore((s) => s.select);

  const activeTool = useToolStore((s) => s.activeTool);
  const options = useToolStore((s) => s.options);
  const tool = activeTool ? getTool(activeTool) : undefined;

  const sessionRef = useRef<ToolSession>(IDLE);

  // `coords` expects the UNROTATED PDF dimensions; `displayed*` are already
  // rotation-swapped for layout, so swap them back for 90°/270°.
  const swapped = ((rotation % 180) + 180) % 180 === 90;
  const geo: PageGeometry = {
    page,
    width: swapped ? displayedHeight : displayedWidth,
    height: swapped ? displayedWidth : displayedHeight,
    scale,
    rotation,
  };
  const cssWidth = displayedWidth * scale;
  const cssHeight = displayedHeight * scale;

  // Escape cancels an in-progress draft.
  useEffect(() => {
    if (!tool) return undefined;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        sessionRef.current = IDLE;
        setDraft(null);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [tool, setDraft]);

  const pagePoint = (e: ReactPointerEvent<Element>) => {
    const rect = e.currentTarget.getBoundingClientRect();
    const s: ScreenPoint = { x: e.clientX - rect.left, y: e.clientY - rect.top };
    return screenToPdf(s, geo);
  };

  const onPointerDown = (e: ReactPointerEvent<Element>) => {
    if (!tool) return;
    e.currentTarget.setPointerCapture?.(e.pointerId);
    const r = stepTool(
      tool,
      sessionRef.current,
      { kind: "pointerDown", pt: pagePoint(e) },
      { documentId, options },
    );
    sessionRef.current = r.session;
    setDraft(r.session.draft);
  };

  const onPointerMove = (e: ReactPointerEvent<Element>) => {
    if (!tool || sessionRef.current.phase !== "drawing") return;
    const r = stepTool(
      tool,
      sessionRef.current,
      { kind: "pointerMove", pt: pagePoint(e) },
      { documentId, options },
    );
    sessionRef.current = r.session;
    setDraft(r.session.draft);
  };

  const onPointerUp = (e: ReactPointerEvent<Element>) => {
    if (!tool || sessionRef.current.phase !== "drawing") return;
    const r = stepTool(
      tool,
      sessionRef.current,
      { kind: "pointerUp", pt: pagePoint(e) },
      { documentId, options },
    );
    sessionRef.current = r.session;
    if (r.committed) addAnnotation(documentId, r.committed);
    else setDraft(null);
  };

  // Notes carry no `/AP` and are drawn by the HTML `NoteLayer` overlay, not as
  // SVG shapes — skip them here so a note never reaches the rect `Shape` branch.
  const pageAnnotations = annotations.filter(
    (a): a is RectAnnotation | MarkupAnnotation => a.page === page && a.type !== "note",
  );
  const draftHere =
    draft && draft.page === page && draft.type !== "note" ? draft : null;

  return (
    <svg
      className="absolute left-0 top-0"
      width={cssWidth}
      height={cssHeight}
      style={{ pointerEvents: tool ? "auto" : "none", cursor: tool?.cursor }}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
    >
      {pageAnnotations.map((a) =>
        a.type === "markup" ? (
          <MarkupShape
            key={a.id}
            markup={a}
            geo={geo}
            selected={a.id === selectedId}
            selectable={!tool}
            onSelect={() => select(a.id)}
          />
        ) : (
          <Shape
            key={a.id}
            id={a.id}
            shape={a}
            geo={geo}
            selected={a.id === selectedId}
            selectable={!tool}
            onSelect={() => select(a.id)}
          />
        ),
      )}
      {draftHere && draftHere.type !== "markup" ? (
        <Shape id="__draft__" shape={draftHere} geo={geo} selected={false} selectable={false} preview />
      ) : null}
    </svg>
  );
}

interface ShapeInput {
  type: "rectangle" | "ellipse";
  rect: PdfRect;
  color: string;
  opacity: number;
  strokeWidth: number;
}

function Shape({
  id,
  shape,
  geo,
  selected,
  selectable,
  onSelect,
  preview = false,
}: {
  id: string;
  shape: ShapeInput;
  geo: PageGeometry;
  selected: boolean;
  selectable: boolean;
  onSelect?: () => void;
  preview?: boolean;
}) {
  const p0 = pdfToScreen({ page: geo.page, x: shape.rect.x0, y: shape.rect.y0 }, geo);
  const p1 = pdfToScreen({ page: geo.page, x: shape.rect.x1, y: shape.rect.y1 }, geo);
  const x = Math.min(p0.x, p1.x);
  const y = Math.min(p0.y, p1.y);
  const w = Math.abs(p1.x - p0.x);
  const h = Math.abs(p1.y - p0.y);

  const stroke = shape.color;
  const strokeWidth = Math.max(1, shape.strokeWidth * geo.scale);
  const common = {
    fill: "none",
    stroke,
    strokeWidth,
    opacity: shape.opacity,
    strokeDasharray: preview ? "4 4" : undefined,
    "data-ann-id": id,
    style: {
      pointerEvents: selectable ? ("auto" as const) : ("none" as const),
      cursor: selectable ? "pointer" : undefined,
    },
    onPointerDown:
      selectable && onSelect
        ? (e: ReactPointerEvent<Element>) => {
            e.stopPropagation();
            onSelect();
          }
        : undefined,
  };

  return (
    <g>
      {shape.type === "ellipse" ? (
        <ellipse cx={x + w / 2} cy={y + h / 2} rx={w / 2} ry={h / 2} {...common} />
      ) : (
        <rect x={x} y={y} width={w} height={h} {...common} />
      )}
      {selected ? (
        <rect
          x={x - 2}
          y={y - 2}
          width={w + 4}
          height={h + 4}
          fill="none"
          stroke="#2563eb"
          strokeWidth={1}
          strokeDasharray="3 3"
          style={{ pointerEvents: "none" }}
        />
      ) : null}
    </g>
  );
}

function quadCorners(q: Quad, geo: PageGeometry) {
  const at = (x: number, y: number) => pdfToScreen({ page: geo.page, x, y }, geo);
  return { ul: at(q[0], q[1]), ur: at(q[2], q[3]), ll: at(q[4], q[5]), lr: at(q[6], q[7]) };
}

function quadScreenBounds(q: Quad, geo: PageGeometry) {
  const c = quadCorners(q, geo);
  const xs = [c.ul.x, c.ur.x, c.ll.x, c.lr.x];
  const ys = [c.ul.y, c.ur.y, c.ll.y, c.lr.y];
  const x = Math.min(...xs);
  const y = Math.min(...ys);
  return { x, y, w: Math.max(...xs) - x, h: Math.max(...ys) - y };
}

function squigglePath(a: ScreenPoint, b: ScreenPoint, amp: number): string {
  const len = Math.hypot(b.x - a.x, b.y - a.y);
  if (len < 1) return `M ${a.x} ${a.y}`;
  const steps = Math.max(2, Math.round(len / (amp * 2)));
  const dx = (b.x - a.x) / steps;
  const dy = (b.y - a.y) / steps;
  const px = (-dy / len) * amp * steps; // perpendicular, scaled per step below
  const py = (dx / len) * amp * steps;
  let d = `M ${a.x} ${a.y}`;
  for (let i = 1; i <= steps; i += 1) {
    const sx = a.x + dx * i;
    const sy = a.y + dy * i;
    const up = i % 2 === 1 ? 1 : -1;
    d += ` L ${sx + (up * px) / steps} ${sy + (up * py) / steps}`;
  }
  return d;
}

function MarkupShape({
  markup,
  geo,
  selected,
  selectable,
  onSelect,
}: {
  markup: MarkupAnnotation;
  geo: PageGeometry;
  selected: boolean;
  selectable: boolean;
  onSelect?: () => void;
}) {
  const interaction = {
    style: {
      pointerEvents: selectable ? ("auto" as const) : ("none" as const),
      cursor: selectable ? "pointer" : undefined,
    },
    onPointerDown:
      selectable && onSelect
        ? (e: ReactPointerEvent<Element>) => {
            e.stopPropagation();
            onSelect();
          }
        : undefined,
  };
  const lineWidth = Math.max(1, geo.scale); // ~1pt

  return (
    <g data-ann-id={markup.id}>
      {markup.quads.map((q, i) => {
        const { ul, ur, ll, lr } = quadCorners(q, geo);
        const key = `${markup.id}-${i}`;
        if (markup.subtype === "highlight") {
          return (
            <polygon
              key={key}
              points={`${ul.x},${ul.y} ${ur.x},${ur.y} ${lr.x},${lr.y} ${ll.x},${ll.y}`}
              fill={markup.color}
              opacity={markup.opacity}
              {...interaction}
              style={{ ...interaction.style, mixBlendMode: "multiply" }}
            />
          );
        }
        if (markup.subtype === "underline") {
          return (
            <line
              key={key}
              x1={ll.x}
              y1={ll.y}
              x2={lr.x}
              y2={lr.y}
              stroke={markup.color}
              strokeWidth={lineWidth}
              {...interaction}
            />
          );
        }
        if (markup.subtype === "strikethrough") {
          return (
            <line
              key={key}
              x1={(ul.x + ll.x) / 2}
              y1={(ul.y + ll.y) / 2}
              x2={(ur.x + lr.x) / 2}
              y2={(ur.y + lr.y) / 2}
              stroke={markup.color}
              strokeWidth={lineWidth}
              {...interaction}
            />
          );
        }
        return (
          <path
            key={key}
            d={squigglePath(ll, lr, Math.max(1.2, geo.scale * 1.5))}
            fill="none"
            stroke={markup.color}
            strokeWidth={lineWidth}
            {...interaction}
          />
        );
      })}
      {selected
        ? markup.quads.map((q, i) => {
            const b = quadScreenBounds(q, geo);
            return (
              <rect
                key={`sel-${markup.id}-${i}`}
                x={b.x - 1}
                y={b.y - 1}
                width={b.w + 2}
                height={b.h + 2}
                fill="none"
                stroke="#2563eb"
                strokeWidth={1}
                strokeDasharray="3 3"
                style={{ pointerEvents: "none" }}
              />
            );
          })
        : null}
    </g>
  );
}
