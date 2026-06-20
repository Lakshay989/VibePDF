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

/** A base-14 font family the free-text tool can request. */
export type FontFamily = "Helvetica" | "Times" | "Courier";

/** Shared, user-configurable style for the active tool. */
export interface ToolOptions {
  /** Stroke / markup / text colour, hex (`#rrggbb`). */
  color: string;
  /** 0..1. */
  opacity: number;
  /** Stroke width in points. */
  strokeWidth: number;
  /** Shape interior colour (P3.C1), hex; `null` = no fill. */
  fillColor: string | null;
  /** Free-text font family (P3.B3). */
  fontFamily: FontFamily;
  /** Free-text font size in points. */
  fontSize: number;
  /** Free-text bold / italic (base-14 variant). */
  bold: boolean;
  italic: boolean;
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

/** The four corners of one marked text rectangle, in PDF points, in the PDF
 *  `/QuadPoints` order: `[x1,y1 (UL), x2,y2 (UR), x3,y3 (LL), x4,y4 (LR)]`. */
export type Quad = [number, number, number, number, number, number, number, number];

export type MarkupSubtype = "highlight" | "underline" | "strikethrough" | "squiggly";

/** A text-markup annotation (P3.B1) — one or more quads over selected text. */
export interface MarkupAnnotation extends AnnotationBase {
  type: "markup";
  subtype: MarkupSubtype;
  quads: Quad[];
}

/** A sticky note (P3.B2) — a PDF `/Text` annotation: an icon at `point` whose
 *  popup shows the editable `content`. `author` is the note's `/T`. */
export interface NoteAnnotation extends AnnotationBase {
  type: "note";
  /** Icon anchor (the note's lower-left), in PDF points. */
  point: { x: number; y: number };
  content: string;
  author: string;
}

/** A line / arrow (P3.C1b) — a segment from `start` to `end`; `arrow` adds an
 *  open-arrow head at `end`. Stroke only (no fill). */
export interface LineAnnotation extends AnnotationBase {
  type: "line";
  start: { x: number; y: number };
  end: { x: number; y: number };
  arrow: boolean;
  strokeWidth: number;
}

/**
 * The committed-annotation union. Grows as concrete tools land (P3.B/C add ink
 * paths, free-text, …).
 */
export type Annotation = RectAnnotation | MarkupAnnotation | NoteAnnotation | LineAnnotation;

/** `Omit` that distributes over a union (plain `Omit` collapses to common keys). */
type DistributiveOmit<T, K extends PropertyKey> = T extends unknown ? Omit<T, K> : never;

/**
 * An in-progress / finished annotation *before* the store assigns its identity.
 * Tools build and return this; the store stamps `id` + `createdAt` on commit,
 * so tools never generate ids. Distributive so each union member keeps its own
 * fields (`rect` for shapes, `quads` for markup).
 */
export type AnnotationDraft = DistributiveOmit<Annotation, "id" | "createdAt">;
