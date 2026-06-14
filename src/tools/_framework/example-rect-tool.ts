// SPEC: infrastructure (P3.A1) — a trivial drag-rectangle tool.
//
// Exists ONLY to exercise the lifecycle + contract in tests; it is not
// registered into the UI. The real shape tools (rectangle / ellipse / …) land
// in P3.C1 and will follow this exact shape. It demonstrates the anchor
// convention: keep the press point as `(x0,y0)` while drawing, normalize only
// on commit.

import type { AnnotationTool, ToolContext } from "./tool-contract";
import { normalizeRect } from "./coords";
import type { AnnotationDraft, PagePoint } from "./types";

export const exampleRectTool: AnnotationTool = {
  id: "rectangle",
  cursor: "crosshair",

  onPointerDown(pt: PagePoint, ctx: ToolContext): AnnotationDraft {
    return {
      type: "rectangle",
      page: pt.page,
      rect: { x0: pt.x, y0: pt.y, x1: pt.x, y1: pt.y },
      color: ctx.options.color,
      opacity: ctx.options.opacity,
      strokeWidth: ctx.options.strokeWidth,
    };
  },

  onPointerMove(draft: AnnotationDraft, pt: PagePoint): AnnotationDraft {
    // This tool only ever produces rect drafts; the guard narrows the union.
    if (draft.type === "markup") return draft;
    // Anchor (x0,y0) stays put; the moving corner is (x1,y1). Not normalized
    // mid-drag, so the anchor survives a drag back across itself.
    return { ...draft, rect: { ...draft.rect, x1: pt.x, y1: pt.y } };
  },

  onPointerUp(draft: AnnotationDraft, pt: PagePoint): AnnotationDraft | null {
    if (draft.type === "markup") return draft;
    const rect = normalizeRect({ ...draft.rect, x1: pt.x, y1: pt.y });
    // Discard a zero-area click (no drag).
    if (rect.x1 - rect.x0 < 1 && rect.y1 - rect.y0 < 1) return null;
    return { ...draft, rect };
  },
};
