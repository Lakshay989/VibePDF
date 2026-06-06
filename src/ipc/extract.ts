import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";
import type { SaveOutcome } from "@/ipc/save";

/**
 * SPEC: P2-PAGE-006 — extract `pages` (0-based indices) of the document
 * into a new PDF at `dest`. Read-only on the source. Returns the write
 * outcome (path + bytes written).
 */
export async function extractPages(
  id: DocumentId,
  pages: number[],
  dest: string,
): Promise<SaveOutcome> {
  return invoke<SaveOutcome>("pdf_extract_pages", { id, pages, dest });
}
