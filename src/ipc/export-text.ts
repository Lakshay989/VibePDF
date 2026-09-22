import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

/** What an export wrote. Mirrors `pdf::export_text::TextExportSummary`. */
export interface TextExportSummary {
  pages: number;
  characters: number;
}

/**
 * SPEC: P7-OCR-006 — write the document's text to `path` as UTF-8, in reading
 * order. Pass an empty `pages` for the whole document.
 *
 * The PDF is not modified. A scanned page has no text to export until OCR has
 * run over it, and comes back as zero characters rather than as an error.
 */
export async function exportText(
  id: DocumentId,
  path: string,
  pages: number[] = [],
): Promise<TextExportSummary> {
  return invoke<TextExportSummary>("pdf_export_text", { id, path, pages });
}
