import type { HistoryState } from "@/ipc/history";
import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

/** A crop rectangle in PDF points (origin bottom-left, y up). */
export interface CropRect {
  left: number;
  bottom: number;
  right: number;
  top: number;
}

/**
 * SPEC: P2-PAGE-009 — crop `page` (0-based) to `rect`. Omit `rect` to
 * reset the CropBox to the MediaBox. Returns the new undo/redo
 * availability so the caller can refresh the Undo button state.
 */
export async function cropPage(
  id: DocumentId,
  page: number,
  rect?: CropRect,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_crop_page", {
    id,
    page,
    left: rect?.left ?? null,
    bottom: rect?.bottom ?? null,
    right: rect?.right ?? null,
    top: rect?.top ?? null,
  });
}
