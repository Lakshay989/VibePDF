import { invoke } from "@/ipc/invoke";

/**
 * SPEC: P2-PAGE-005 (support) — read a file's page count without opening it as
 * a document. Read-only; used by the insert-from dialog to show the source's
 * length and validate the chosen page range.
 */
export async function peekPageCount(path: string): Promise<number> {
  return invoke<number>("pdf_peek_page_count", { path });
}
