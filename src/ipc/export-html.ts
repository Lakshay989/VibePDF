import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

/** What an export produced. Mirrors `pdf::export_html::HtmlExportSummary`. */
export interface HtmlExportSummary {
  pages: number;
  paragraphs: number;
  headings: number;
  images: number;
  tables: number;
  /** Size of the `.html` file alone. */
  bytes: number;
  /** The folder beside the `.html` the images went to; `null` when there were none. */
  imagesFolder: string | null;
}

/**
 * SPEC: P7-OCR-007 — write the document to `path` as semantic HTML: headings,
 * paragraphs, bold/italic and tables as their elements, in reading order. Images
 * go to `<name>_files/` beside the file. Pass an empty `pages` for the whole
 * document.
 *
 * The page loads nothing from the network. The PDF is not modified.
 */
export async function exportHtml(
  id: DocumentId,
  path: string,
  pages: number[] = [],
): Promise<HtmlExportSummary> {
  return invoke<HtmlExportSummary>("pdf_export_html", { id, path, pages });
}
