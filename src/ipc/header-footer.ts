import { type HistoryState } from "@/ipc/history";
import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

export interface HeaderFooterArgs {
  /** "header" (top margin) or "footer" (bottom margin). */
  position: "header" | "footer";
  /** Templates for each position; empty ones are skipped. `{n}` / `{total}` /
   *  `{date}` are substituted per page. */
  left: string;
  center: string;
  right: string;
  fontFamily: string;
  fontSize: number;
  color: string;
  /** Points from the page edge. */
  margin: number;
  /** The `{date}` value — the frontend's locale-formatted today. */
  date: string;
}

/**
 * SPEC: P4-EDIT-010 (P4.D3) — draw a header/footer on `pages` (0-based). Returns
 * the new undo/redo availability.
 */
export async function addHeaderFooter(
  id: DocumentId,
  pages: number[],
  args: HeaderFooterArgs,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_add_header_footer", { id, pages, ...args });
}
