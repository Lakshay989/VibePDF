import { type HistoryState } from "@/ipc/history";
import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

/** A text-box rectangle in PDF points: `[x0, y0, x1, y1]` (origin bottom-left). */
export type TextBoxRect = [number, number, number, number];

/**
 * SPEC: P4-EDIT-003 (P4.B2) — add a text box as **page content** (not an
 * annotation): the text is appended to the page's content stream, so it becomes
 * ordinary selectable text (editable/deletable via P4.B1/B3). Goes through the
 * document actor; returns the new undo/redo availability.
 */
export async function addTextBox(
  id: DocumentId,
  page: number,
  rect: TextBoxRect,
  text: string,
  fontFamily: string,
  fontSize: number,
  color: string,
  bold: boolean,
  italic: boolean,
  underline: boolean,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_add_text_box", {
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

/** One re-editable Add-Text box read back from a page's content stream (P4-EDIT-003b). */
export interface TextBoxInfo {
  /** The box's marked-content `/Id` — the re-edit key. */
  id: string;
  /** The rectangle `[x0, y0, x1, y1]` the box occupies, in PDF points. */
  rect: TextBoxRect;
  /** The original source text (with the user's newlines). */
  text: string;
  fontFamily: string;
  fontSize: number;
  color: string;
  bold: boolean;
  italic: boolean;
  underline: boolean;
}

/**
 * SPEC: P4-EDIT-003b — list every re-editable Add-Text box on `page` (0-based).
 * Read-only; powers double-click re-edit hit-testing. Boxes not written by this
 * app (no marked-content tag) are not returned.
 */
export async function readTextBoxes(id: DocumentId, page: number): Promise<TextBoxInfo[]> {
  return invoke<TextBoxInfo[]>("pdf_read_text_boxes", { id, page });
}

/**
 * SPEC: P4-EDIT-003b — re-edit the Add-Text box `boxId` on `page`: replace its
 * text + style, preserving its rectangle. Goes through the document actor as one
 * undoable edit; returns the new undo/redo availability.
 */
export async function updateTextBox(
  id: DocumentId,
  page: number,
  boxId: string,
  text: string,
  fontFamily: string,
  fontSize: number,
  color: string,
  bold: boolean,
  italic: boolean,
  underline: boolean,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_update_text_box", {
    id,
    page,
    boxId,
    text,
    fontFamily,
    fontSize,
    color,
    bold,
    italic,
    underline,
  });
}
