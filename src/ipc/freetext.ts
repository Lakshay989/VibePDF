import type { HistoryState } from "@/ipc/history";
import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";
import type { FontFamily } from "@/tools/_framework";

/** A free-text box in PDF points: `[x0, y0, x1, y1]`, lower-left first. */
export type FreeTextRect = [number, number, number, number];

/** A free-text annotation's editable state. Mirrors `cos::FreeTextData` (Rust). */
export interface FreeTextData {
  rect: FreeTextRect;
  text: string;
  fontFamily: FontFamily;
  fontSize: number;
  color: string;
  bold: boolean;
  italic: boolean;
  underline: boolean;
}

/**
 * SPEC: P3-ANN-013 — read a free-text annotation's text + style by its `/NM`, so
 * the in-place editor can open pre-filled. `null` if there's no such free-text.
 * Read-only; runs on the Rust document actor.
 */
export async function readFreeText(
  id: DocumentId,
  nm: string,
): Promise<FreeTextData | null> {
  return invoke<FreeTextData | null>("pdf_read_free_text", { id, nm });
}

/**
 * SPEC: P3-ANN-013 — update a free-text annotation (by `/NM`) in place: new text
 * + style, preserving its identity. Undoable; runs on the Rust document actor.
 */
export async function updateFreeText(
  id: DocumentId,
  nm: string,
  text: string,
  fontFamily: FontFamily,
  fontSize: number,
  color: string,
  bold: boolean,
  italic: boolean,
  underline: boolean,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_update_free_text", {
    id,
    nm,
    text,
    fontFamily,
    fontSize,
    color,
    bold,
    italic,
    underline,
  });
}

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
  underline: boolean,
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
    underline,
  });
}
