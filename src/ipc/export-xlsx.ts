import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

/**
 * What an export produced. Mirrors `pdf::export_xlsx::XlsxExportSummary`.
 *
 * `tables === 0` is P7-OCR-008's warning case, and then **no file was
 * written** — there is nothing at the chosen path to open.
 */
export interface XlsxExportSummary {
  pages: number;
  tables: number;
  cells: number;
  bytes: number;
}

/**
 * SPEC: P7-OCR-008 — write the document's detected tables to `path` as an Excel
 * `.xlsx`, one sheet a table. Pass an empty `pages` for the whole document.
 *
 * Only **ruled** tables are detected: a table the page draws a grid for. One
 * laid out with whitespace alone is invisible here by design, because guessing
 * would turn two-column prose into a spreadsheet. The PDF is not modified.
 */
export async function exportXlsx(
  id: DocumentId,
  path: string,
  pages: number[] = [],
): Promise<XlsxExportSummary> {
  return invoke<XlsxExportSummary>("pdf_export_xlsx", { id, path, pages });
}
