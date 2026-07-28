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

import { reportError } from "@/app/report-error";
import { type PointerEvent as ReactPointerEvent, useEffect, useRef } from "react";

import { addLine } from "@/ipc/lines";
import { addShape } from "@/ipc/shapes";
import { useAnnotationStore, useDocAnnotations } from "@/state/annotation-store";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useHistoryStore } from "@/state/history-store";
import { useOptimisticEditStore, usePendingEdits } from "@/state/optimistic-edit-store";
import { useToolStore } from "@/state/tool-store";
import {
  type AnnotationDraft,
  getTool,
  IDLE,
  type MarkupAnnotation,
  type PageGeometry,
  type RectAnnotation,
  type PdfRect,
  pdfToScreen,
  type Quad,
  registerTool,
  type ScreenPoint,
  screenToPdf,
  stepTool,
  type ToolSession,
} from "@/tools/_framework";
import { lineTools } from "@/tools/shapes/line-tools";
import { shapeTools } from "@/tools/shapes/shape-tools";

// Register the shape + line tools once, when this module loads. The overlay is
// their host: it drives the gesture and persists the committed draft (below).
for (const tool of [...shapeTools, ...lineTools]) registerTool(tool);

/** Optimistic-preview payload for a committed shape awaiting bake (P4.HF29). */
type ShapeHeld =
  | {
      variant: "line";
      start: { x: number; y: number };
      end: { x: number; y: number };
      arrow: boolean;
      color: string;
      opacity: number;
      strokeWidth: number;
    }
  | {
      variant: "shape";
      type: "rectangle" | "ellipse";
      rect: PdfRect;
      color: string;
      opacity: number;
      strokeWidth: number;
    };

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
  const bumpEpoch = useEditEpochStore((s) => s.bumpEpoch);
  const setHistory = useHistoryStore((s) => s.setHistory);

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
    if (r.committed) commitDraft(r.committed);
    else setDraft(null);
  };

  // Persist a committed draft. Shapes (rectangle/ellipse) are written to the PDF
  // via the actor (lopdf builds the `/AP`); the canvas then renders them on the
  // epoch reload — so the store holds only the in-progress draft, not committed
  // shapes. Any non-shape draft falls back to the store (preview-only tools).
  const commitDraft = (committed: AnnotationDraft) => {
    const persisted = (h: Parameters<typeof setHistory>[1]) => {
      bumpEpoch(documentId);
      setHistory(documentId, h);
    };
    const oe = useOptimisticEditStore.getState();
    if (committed.type === "line") {
      const { start, end, arrow } = committed;
      setDraft(null);
      // Show the committed line/arrow immediately (P4.HF29) — the ~3 s apply +
      // reload on a large PDF would otherwise blank it until the bake lands.
      const key = oe.add(documentId, committed.page, "shape", {
        variant: "line",
        start,
        end,
        arrow,
        color: options.color,
        opacity: options.opacity,
        strokeWidth: options.strokeWidth,
      } satisfies ShapeHeld);
      addLine(
        documentId,
        committed.page,
        start.x,
        start.y,
        end.x,
        end.y,
        arrow,
        options.color,
        options.opacity,
        options.strokeWidth,
      )
        .then((h) => {
          persisted(h);
          oe.tie(documentId, key, useEditEpochStore.getState().byDoc[documentId] ?? 0);
        })
        .catch((err: unknown) => {
          oe.remove(documentId, key);
          reportError("Couldn't add line", err);
        });
      return;
    }
    if (committed.type !== "rectangle" && committed.type !== "ellipse") {
      addAnnotation(documentId, committed);
      return;
    }
    const { rect } = committed;
    setDraft(null);
    const key = oe.add(documentId, committed.page, "shape", {
      variant: "shape",
      type: committed.type,
      rect,
      color: options.color,
      opacity: options.opacity,
      strokeWidth: options.strokeWidth,
    } satisfies ShapeHeld);
    addShape(
      documentId,
      committed.page,
      committed.type,
      [rect.x0, rect.y0, rect.x1, rect.y1],
      options.color,
      options.fillColor,
      options.opacity,
      options.strokeWidth,
    )
      .then((h) => {
        persisted(h);
        oe.tie(documentId, key, useEditEpochStore.getState().byDoc[documentId] ?? 0);
      })
      .catch((err: unknown) => {
        oe.remove(documentId, key);
        reportError("Couldn't add shape", err);
      });
  };

  // Notes carry no `/AP` and are drawn by the HTML `NoteLayer` overlay, not as
  // SVG shapes — skip them here so a note never reaches the rect `Shape` branch.
  const pageAnnotations = annotations.filter(
    (a): a is RectAnnotation | MarkupAnnotation => a.page === page && a.type !== "note",
  );
  const draftHere =
    draft && draft.page === page && draft.type !== "note" ? draft : null;
  const pendingShapes = usePendingEdits<ShapeHeld>(documentId, page, "shape");

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
      {draftHere && (draftHere.type === "rectangle" || draftHere.type === "ellipse") ? (
        <Shape id="__draft__" shape={draftHere} geo={geo} selected={false} selectable={false} preview />
      ) : null}
      {draftHere && draftHere.type === "line" ? <LineShape line={draftHere} geo={geo} /> : null}

      {/* Optimistic preview: committed shapes not yet baked into the page (P4.HF29).
          Drawn solid (not the dashed draft) so they read as done — "solid, then swap". */}
      {pendingShapes.map(({ key, data }) =>
        data.variant === "line" ? (
          <LineShape key={key} line={data} geo={geo} />
        ) : (
          <Shape key={key} id={key} shape={data} geo={geo} selected={false} selectable={false} />
        ),
      )}
    </svg>
  );
}

/** The live line/arrow draft preview (committed lines are canvas-rendered). */
function LineShape({
  line,
  geo,
}: {
  line: {
    start: { x: number; y: number };
    end: { x: number; y: number };
    arrow: boolean;
    color: string;
    opacity: number;
    strokeWidth: number;
  };
  geo: PageGeometry;
}) {
  const a = pdfToScreen({ page: geo.page, x: line.start.x, y: line.start.y }, geo);
  const b = pdfToScreen({ page: geo.page, x: line.end.x, y: line.end.y }, geo);
  const w = Math.max(1, line.strokeWidth * geo.scale);

  let head: string | null = null;
  if (line.arrow) {
    const dx = b.x - a.x;
    const dy = b.y - a.y;
    const len = Math.hypot(dx, dy);
    if (len > 1) {
      const ux = dx / len;
      const uy = dy / len;
      const hl = Math.max(8, w * 3);
      const hw = hl * 0.9;
      const bx = b.x - ux * hl;
      const by = b.y - uy * hl;
      const px = -uy;
      const py = ux;
      head = `${bx + (px * hw) / 2},${by + (py * hw) / 2} ${b.x},${b.y} ${bx - (px * hw) / 2},${by - (py * hw) / 2}`;
    }
  }

  return (
    <g>
      <line
        x1={a.x}
        y1={a.y}
        x2={b.x}
        y2={b.y}
        stroke={line.color}
        strokeWidth={w}
        opacity={line.opacity}
        strokeDasharray="4 4"
        style={{ pointerEvents: "none" }}
      />
      {head ? (
        <polyline
          points={head}
          fill="none"
          stroke={line.color}
          strokeWidth={w}
          opacity={line.opacity}
          style={{ pointerEvents: "none" }}
        />
      ) : null}
    </g>
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
