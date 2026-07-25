import { type HistoryState } from "@/ipc/history";
import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

/**
 * SPEC: P4-EDIT-009 (P4.D2) — stamp a **text** watermark on `pages` (0-based) at
 * `opacity` (0..1) and `rotation` degrees, `behind` content or on top. Returns
 * the new undo/redo availability.
 */
export async function addTextWatermark(
  id: DocumentId,
  pages: number[],
  text: string,
  fontFamily: string,
  fontSize: number,
  color: string,
  opacity: number,
  rotation: number,
  behind: boolean,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_add_text_watermark", {
    id,
    pages,
    text,
    fontFamily,
    fontSize,
    color,
    opacity,
    rotation,
    behind,
  });
}

/**
 * SPEC: P4-EDIT-009 (P4.D2) — stamp an **image** watermark (PNG/JPEG at
 * `imagePath`) on `pages`. The Rust command reads the file; the frontend never
 * touches the bytes.
 */
export async function addImageWatermark(
  id: DocumentId,
  pages: number[],
  imagePath: string,
  opacity: number,
  rotation: number,
  behind: boolean,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_add_image_watermark", {
    id,
    pages,
    imagePath,
    opacity,
    rotation,
    behind,
  });
}

/**
 * SPEC: P4-EDIT-009 — remove every watermark this app added, from all pages, as
 * one undoable edit. Foreign watermarks (no VibePDF tag) are left alone. Returns
 * the new undo/redo availability.
 */
export async function removeWatermarks(id: DocumentId): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_remove_watermarks", { id });
}
