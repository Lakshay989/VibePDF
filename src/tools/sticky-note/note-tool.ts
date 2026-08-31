// SPEC: P3-ANN-002 (P3.B2a) — the sticky-note placement tool.
//
// A note is a click-to-place annotation, not a drag gesture: a single press
// drops a `/Text` annotation at that PDF point and opens its popup for editing.
// Unlike the shape/markup tools this one is NOT driven through the generic
// AnnotationLayer lifecycle — notes need an HTML popup and direct persistence,
// so `NoteLayer` (src/view/note-layer.tsx) owns the gesture and calls
// `noteDraftAt` here. The `AnnotationTool` shape is kept so placement stays a
// pure, DOM-free, unit-testable reducer (see __tests__).

import type { AnnotationTool, ToolContext } from "@/tools/_framework/tool-contract";
import type { AnnotationDraft, PagePoint } from "@/tools/_framework/types";

/** Default `/T` (author) stamped on a new note until per-user identity exists. */
export const DEFAULT_NOTE_AUTHOR = "VibePDF User";

/** The note icon colour — matches the backend `/C [1 0.82 0]` (a mainstream reader yellow). */
export const NOTE_COLOR = "#ffd200";

/** Build an empty note draft anchored at `(x, y)` (PDF points, lower-left). */
export function noteDraftAt(page: number, x: number, y: number, author: string): AnnotationDraft {
  return {
    type: "note",
    page,
    point: { x, y },
    content: "",
    author,
    color: NOTE_COLOR,
    opacity: 1,
  };
}

export const stickyNoteTool: AnnotationTool = {
  id: "sticky-note",
  cursor: "copy",

  onPointerDown(pt: PagePoint): AnnotationDraft {
    return noteDraftAt(pt.page, pt.x, pt.y, DEFAULT_NOTE_AUTHOR);
  },

  // A note is placed on press; move/up just carry the draft unchanged.
  onPointerMove(draft: AnnotationDraft): AnnotationDraft {
    return draft;
  },

  onPointerUp(draft: AnnotationDraft, _pt: PagePoint, _ctx: ToolContext): AnnotationDraft {
    return draft;
  },
};
