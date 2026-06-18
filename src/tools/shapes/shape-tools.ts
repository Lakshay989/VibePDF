// SPEC: P3-ANN-004 (P3.C1a) — the drag-to-size shape tools (rectangle, ellipse).
//
// Both are the same gesture — press, drag the opposite corner, release — so a
// factory parameterizes the draft `type`. Follows the `example-rect-tool`
// template (kept as the framework's pure lifecycle fixture); the committed draft
// is persisted by `annotation-layer` (write → /AP → canvas), not stored.
//
// Fill / stroke / opacity / width are read from the live tool options at commit
// time, so the draft carries only the geometry + stroke preview.

import { normalizeRect } from "@/tools/_framework";
import type { AnnotationTool, ToolContext } from "@/tools/_framework/tool-contract";
import type { AnnotationDraft, PagePoint } from "@/tools/_framework/types";

type ShapeKind = "rectangle" | "ellipse";

const isShape = (d: AnnotationDraft): d is AnnotationDraft & { type: ShapeKind; rect: { x0: number; y0: number; x1: number; y1: number } } =>
  d.type === "rectangle" || d.type === "ellipse";

function makeDragRectTool(kind: ShapeKind): AnnotationTool {
  return {
    id: kind,
    cursor: "crosshair",

    onPointerDown(pt: PagePoint, ctx: ToolContext): AnnotationDraft {
      return {
        type: kind,
        page: pt.page,
        rect: { x0: pt.x, y0: pt.y, x1: pt.x, y1: pt.y },
        color: ctx.options.color,
        opacity: ctx.options.opacity,
        strokeWidth: ctx.options.strokeWidth,
      };
    },

    onPointerMove(draft: AnnotationDraft, pt: PagePoint): AnnotationDraft {
      if (!isShape(draft)) return draft;
      // Anchor (x0,y0) stays; the moving corner is (x1,y1). Normalized on commit.
      return { ...draft, rect: { ...draft.rect, x1: pt.x, y1: pt.y } };
    },

    onPointerUp(draft: AnnotationDraft, pt: PagePoint): AnnotationDraft | null {
      if (!isShape(draft)) return draft;
      const rect = normalizeRect({ ...draft.rect, x1: pt.x, y1: pt.y });
      // Discard a zero-area click (no real drag).
      if (rect.x1 - rect.x0 < 1 && rect.y1 - rect.y0 < 1) return null;
      return { ...draft, rect };
    },
  };
}

export const rectangleTool = makeDragRectTool("rectangle");
export const ellipseTool = makeDragRectTool("ellipse");

/** The shape tools registered into the UI. */
export const shapeTools: readonly AnnotationTool[] = [rectangleTool, ellipseTool];
