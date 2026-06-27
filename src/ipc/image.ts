import { type HistoryState } from "@/ipc/history";
import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

/** An image-box rectangle in PDF points: `[x0, y0, x1, y1]` (origin bottom-left). */
export type ImageRect = [number, number, number, number];

/**
 * SPEC: P4-EDIT-005 (P4.C1) — embed the image at `imagePath` (PNG or JPEG) as
 * **page content** (an Image XObject painted by `Do`), aspect-fit into `rect`.
 * The Rust command reads the file; the frontend never touches the bytes. Returns
 * the new undo/redo availability.
 */
export async function addImage(
  id: DocumentId,
  page: number,
  rect: ImageRect,
  imagePath: string,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_add_image", { id, page, rect, imagePath });
}
