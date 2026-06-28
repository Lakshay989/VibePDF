import { type HistoryState } from "@/ipc/history";
import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";
import type { Matrix } from "@/tools/image-edit/matrix";

/** One image on a page, mirroring `image_extract::ImageInfo` (Rust). */
export interface ImageInfo {
  /** 0-based image ordinal on the page (the handle the edit/delete commands take). */
  index: number;
  /** Axis-aligned bounds `[x0, y0, x1, y1]` in PDF points (origin bottom-left). */
  bbox: [number, number, number, number];
  /** The image's placement matrix `[a, b, c, d, e, f]`. */
  matrix: Matrix;
}

/**
 * SPEC: P4-EDIT-006 (P4.C2) — locate the images on `page` (0-based) for
 * click-to-select. Read-only; runs on the Rust actor against the live document.
 */
export async function extractImages(id: DocumentId, page: number): Promise<ImageInfo[]> {
  return invoke<ImageInfo[]>("pdf_extract_images", { id, page });
}

/**
 * SPEC: P4-EDIT-006 (P4.C2) — override image `index`'s placement matrix
 * (move/resize/rotate). Goes through the actor; returns undo/redo availability.
 */
export async function transformImage(
  id: DocumentId,
  page: number,
  index: number,
  matrix: Matrix,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_transform_image", { id, page, index, matrix });
}

/** SPEC: P4-EDIT-006 (P4.C2) — delete image `index` on `page`. */
export async function deleteImage(
  id: DocumentId,
  page: number,
  index: number,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_delete_image", { id, page, index });
}
