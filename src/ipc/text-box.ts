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
