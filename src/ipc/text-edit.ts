import { type HistoryState } from "@/ipc/history";
import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

/**
 * SPEC: P4-EDIT-001 (P4.B1) — replace text run `runIndex` on `page` (0-based, the
 * same ordering `extractTextRuns` returns) with `newText`, preserving the run's
 * font/size/colour/matrix. Goes through the Rust document actor (the frontend
 * never writes bytes); returns the new undo/redo availability.
 */
export async function replaceTextRun(
  id: DocumentId,
  page: number,
  runIndex: number,
  newText: string,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_replace_text_run", { id, page, runIndex, newText });
}

/**
 * SPEC: P4-EDIT-004 (P4.B3) — remove text run `runIndex` on `page` from the page
 * content stream entirely (lopdf splice, verified by re-extraction). Through the
 * document actor; returns the new undo/redo availability.
 */
export async function deleteTextRun(
  id: DocumentId,
  page: number,
  runIndex: number,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_delete_text_run", { id, page, runIndex });
}
