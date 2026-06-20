// SPEC: P3-ANN-004 (P3.C1b₁) — the drag line + arrow tools.
//
// A line is a two-point drag: press = start, release = end. The arrow tool is
// the same gesture with `arrow: true` (an open-arrow head at the end). Follows
// the `makeDragRectTool` template; the committed draft is persisted by
// `annotation-layer` (write → /AP → canvas), not stored.

import type { AnnotationTool, ToolContext } from "@/tools/_framework/tool-contract";
import type { AnnotationDraft, PagePoint } from "@/tools/_framework/types";

/** Minimum drag length (PDF pts) to keep — a shorter drag is a stray click. */
const MIN_LEN = 1;

const isLine = (
  d: AnnotationDraft,
): d is AnnotationDraft & {
  type: "line";
  start: { x: number; y: number };
  end: { x: number; y: number };
} => d.type === "line";

function makeLineTool(id: "line" | "arrow", arrow: boolean): AnnotationTool {
  return {
    id,
    cursor: "crosshair",

    onPointerDown(pt: PagePoint, ctx: ToolContext): AnnotationDraft {
      return {
        type: "line",
        page: pt.page,
        start: { x: pt.x, y: pt.y },
        end: { x: pt.x, y: pt.y },
        arrow,
        color: ctx.options.color,
        opacity: ctx.options.opacity,
        strokeWidth: ctx.options.strokeWidth,
      };
    },

    onPointerMove(draft: AnnotationDraft, pt: PagePoint): AnnotationDraft {
      if (!isLine(draft)) return draft;
      return { ...draft, end: { x: pt.x, y: pt.y } };
    },

    onPointerUp(draft: AnnotationDraft, pt: PagePoint): AnnotationDraft | null {
      if (!isLine(draft)) return draft;
      const end = { x: pt.x, y: pt.y };
      if (Math.hypot(end.x - draft.start.x, end.y - draft.start.y) < MIN_LEN) return null;
      return { ...draft, end };
    },
  };
}

export const lineTool = makeLineTool("line", false);
export const arrowTool = makeLineTool("arrow", true);

/** The line tools registered into the UI. */
export const lineTools: readonly AnnotationTool[] = [lineTool, arrowTool];
