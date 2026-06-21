import type { HistoryState } from "@/ipc/history";
import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

/**
 * SPEC: P3-ANN-006 — add a `/Stamp` annotation bounded by `rect` (PDF points) on
 * `page` (0-based). The write runs on the Rust document actor (lopdf, with a
 * generated `/AP`) — the frontend never touches PDF bytes. Returns the new
 * undo/redo availability.
 */
export async function addStamp(
  id: DocumentId,
  page: number,
  rect: [number, number, number, number],
  text: string,
  name: string,
  color: string,
  opacity: number,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_add_stamp", { id, page, rect, text, name, color, opacity });
}
