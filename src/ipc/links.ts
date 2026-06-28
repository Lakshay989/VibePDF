import { type HistoryState } from "@/ipc/history";
import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

/** A link rectangle in PDF points: `[x0, y0, x1, y1]` (origin bottom-left). */
export type LinkRect = [number, number, number, number];

/**
 * SPEC: P4-EDIT-007 (P4.C3) — add a `/Link` annotation over `rect` on `page`
 * (0-based). `kind` is `url` | `email` | `page` | `named`; `value` is the
 * matching target (URL / address / 0-based target-page index / destination
 * name). Returns the new undo/redo availability.
 */
export async function addLink(
  id: DocumentId,
  page: number,
  rect: LinkRect,
  kind: string,
  value: string,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_add_link", { id, page, rect, kind, value });
}
