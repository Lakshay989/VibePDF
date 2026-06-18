import type { HistoryState } from "@/ipc/history";
import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";
import type { FontFamily } from "@/tools/_framework";

/** A free-text box in PDF points: `[x0, y0, x1, y1]`, lower-left first. */
export type FreeTextRect = [number, number, number, number];

/**
 * SPEC: P3-ANN-003 — add a free-text annotation: a `/FreeText` box at `rect`
 * (PDF points) on `page` (0-based) holding `text` in a base-14 font. The write
 * runs on the Rust document actor (lopdf, with a generated `/AP` appearance) —
 * the frontend never touches PDF bytes. Returns the new undo/redo availability.
 */
export async function addFreeText(
  id: DocumentId,
  page: number,
  rect: FreeTextRect,
  text: string,
  fontFamily: FontFamily,
  fontSize: number,
  color: string,
  bold: boolean,
  italic: boolean,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_add_free_text", {
    id,
    page,
    rect,
    text,
    fontFamily,
    fontSize,
    color,
    bold,
    italic,
  });
}
