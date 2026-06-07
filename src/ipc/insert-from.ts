import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";
import type { HistoryState } from "@/ipc/history";

/**
 * SPEC: P2-PAGE-005 — insert `pages` (0-based indices into `sourcePath`) into
 * the open document at `index` (0-based; `index === pageCount` appends).
 * Undoable. Copies content, page annotations, and dimensions; interactive
 * form fields are not carried yet (deferred to the lopdf follow-up). The write
 * runs on the Rust document actor — the frontend never touches PDF bytes.
 */
export async function insertPagesFromPdf(
  id: DocumentId,
  sourcePath: string,
  pages: number[],
  index: number,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_insert_from_pdf", {
    id,
    sourcePath,
    pages,
    index,
  });
}
