import { type HistoryState } from "@/ipc/history";
import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

/**
 * SPEC: P4-EDIT-008 (P4.D1) — fill `pages` (0-based) behind their content with a
 * solid `color` (`#rrggbb`) at `opacity` (0..1). Returns the new undo/redo
 * availability.
 */
export async function addColorBackground(
  id: DocumentId,
  pages: number[],
  color: string,
  opacity: number,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_add_color_background", { id, pages, color, opacity });
}

/**
 * SPEC: P4-EDIT-008 (P4.D1) — fill `pages` behind their content with an image
 * (PNG/JPEG at `imagePath`), cover-fit. The Rust command reads the file; the
 * frontend never touches the bytes.
 */
export async function addImageBackground(
  id: DocumentId,
  pages: number[],
  imagePath: string,
  opacity: number,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_add_image_background", { id, pages, imagePath, opacity });
}

/**
 * SPEC: P4-EDIT-008 (P4.D1b) — fill `pages` behind their content with the
 * 0-based `sourcePage` of the PDF at `sourcePath`, imported as a Form XObject
 * (contain-fit). The Rust command reads the source file.
 */
export async function addPdfBackground(
  id: DocumentId,
  pages: number[],
  sourcePath: string,
  sourcePage: number,
  opacity: number,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_add_pdf_background", {
    id,
    pages,
    sourcePath,
    sourcePage,
    opacity,
  });
}
