import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";
import type { HistoryState } from "@/ipc/history";

/**
 * SPEC: P2-PAGE-010 — resize `pages` (0-based) to `width` × `height` points,
 * scaling content to fit. `preserveAspect` scales the content uniformly by the
 * smaller ratio and centres it; otherwise content is stretched to fill.
 * Undoable. The write runs on the Rust document actor — the frontend never
 * touches PDF bytes.
 */
export async function resizePages(
  id: DocumentId,
  pages: number[],
  width: number,
  height: number,
  preserveAspect: boolean,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_resize_pages", {
    id,
    pages,
    width,
    height,
    preserveAspect,
  });
}
