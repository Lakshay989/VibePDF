// SPEC: infrastructure (P3.A1) — the annotation domain model shared by the tool
// framework (this dir) and the annotation store (`src/state/annotation-store`).
//
// Annotations live in PDF coordinate space (origin bottom-left, y up, units =
// points = 1/72"). The union grows as concrete tools land (P3.B/C); for the
// framework step it carries the shapes the example tool needs.

/** A point in a page's PDF coordinate space (origin bottom-left, y up). */
export interface PagePoint {
  /** 0-based page index. */
  page: number;
  x: number;
  y: number;
}

/** An axis-aligned rectangle in PDF points (`x0,y0` = lower-left). */
export interface PdfRect {
  x0: number;
  y0: number;
  x1: number;
  y1: number;
}

/** Identifier for a registered tool. Extended as concrete tools land. */
export type ToolId =
  | "select"
  | "highlight"
  | "underline"
  | "strikethrough"
  | "squiggly"
  | "sticky-note"
  | "free-text"
  | "rectangle"
  | "ellipse"
  | "line"
  | "arrow"
  | "polygon"
  | "ink"
  | "stamp"
  | "measure";

/** Shared, user-configurable style for the active tool. */
export interface ToolOptions {
  /** Stroke / markup colour, hex (`#rrggbb`). */
  color: string;
  /** 0..1. */
  opacity: number;
  /** Stroke width in points. */
  strokeWidth: number;
}

/** Fields every annotation carries. */
export interface AnnotationBase {
  id: string;
  /** 0-based page index. */
  page: number;
  color: string;
  opacity: number;
  author?: string;
  /** Epoch milliseconds. */
  createdAt: number;
}

/** A rectangle / ellipse shape annotation (the example + future P3.C1 tools). */
export interface RectAnnotation extends AnnotationBase {
  type: "rectangle" | "ellipse";
  rect: PdfRect;
  strokeWidth: number;
}

/**
 * The committed-annotation union. For the framework step it has a single
 * member; P3.B/C extend it (highlight quads, ink paths, free-text, …).
 */
export type Annotation = RectAnnotation;

/**
 * An in-progress / finished annotation *before* the store assigns its identity.
 * Tools build and return this; the store stamps `id` + `createdAt` on commit,
 * so tools never generate ids.
 */
export type AnnotationDraft = Omit<Annotation, "id" | "createdAt">;
