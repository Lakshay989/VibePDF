import type { HistoryState } from "@/ipc/history";
import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";
import type { MarkupSubtype, Quad } from "@/tools/_framework";

/**
 * SPEC: P3-ANN-001 — add a text-markup annotation over `quads` (each
 * `[x1..y4]` in PDF points) on `page` (0-based). The write runs on the Rust
 * document actor (lopdf, with a generated `/AP` appearance) — the frontend
 * never touches PDF bytes. Returns the new undo/redo availability.
 */
export async function addTextMarkup(
  id: DocumentId,
  page: number,
  subtype: MarkupSubtype,
  quads: Quad[],
  color: string,
  opacity: number,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_add_text_markup", {
    id,
    page,
    subtype,
    quads,
    color,
    opacity,
  });
}

/**
 * SPEC: P3-ANN-001 — remove all text-markup annotations from the document.
 * Undoable. Runs on the Rust document actor.
 */
export async function clearTextMarkup(id: DocumentId): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_clear_text_markup", { id });
}

/** The annotation kinds the sidebar surfaces. Mirrors `cos::annotation_kind`. */
export type AnnotationKind =
  | "highlight"
  | "underline"
  | "strikeout"
  | "squiggly"
  | "note"
  | "freetext"
  | "rectangle"
  | "ellipse"
  | "line"
  | "polygon"
  | "polyline"
  | "ink"
  | "stamp"
  | "measure";

/**
 * SPEC: P3-ANN-012 — delete the annotation with the given `handle` (its `/NM`,
 * or an `obj:<num> <gen>` id for one authored elsewhere). Undoable; runs on the
 * Rust document actor. Re-exported here so the annotation panel needn't import
 * from `ipc/notes` — the command is generic over annotation kind.
 */
export { deleteAnnotation } from "@/ipc/notes";

/** One annotation as the sidebar sees it. Mirrors `cos::AnnotationInfo` (Rust). */
export interface AnnotationInfo {
  /** Stable within this load (the lopdf object id) — used to track selection. */
  id: string;
  /** 0-based page index. */
  page: number;
  kind: AnnotationKind;
  /** `/Rect` bounds `[x0, y0, x1, y1]` in PDF points (for the selection highlight). */
  rect: [number, number, number, number];
  contents: string;
  author: string;
  /** `/M` parsed to epoch milliseconds, or null when absent/unparsable. */
  modified: number | null;
}

/**
 * SPEC: P3-ANN-008 — read every supported annotation out of the open document
 * for the sidebar list. Read-only (no undo entry); the panel pulls this on open
 * and after each edit epoch. Runs on the Rust document actor.
 */
export async function readAnnotations(id: DocumentId): Promise<AnnotationInfo[]> {
  return invoke<AnnotationInfo[]>("pdf_read_annotations", { id });
}
