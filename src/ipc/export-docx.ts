import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

/** What an export produced. Mirrors `pdf::export_docx::DocxExportSummary`. */
export interface DocxExportSummary {
  pages: number;
  paragraphs: number;
  headings: number;
  images: number;
  bytes: number;
}

/**
 * SPEC: P7-OCR-004 — write the document to `path` as a Word `.docx`,
 * preserving reading order, bold/italic, headings and images. Pass an empty
 * `pages` for the whole document.
 *
 * The PDF is not modified. Tables are not yet detected (P7.B1b); a table comes
 * out as the text it contains, in reading order.
 */
export async function exportDocx(
  id: DocumentId,
  path: string,
  pages: number[] = [],
): Promise<DocxExportSummary> {
  return invoke<DocxExportSummary>("pdf_export_docx", { id, path, pages });
}
