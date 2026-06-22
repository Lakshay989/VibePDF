import type { HistoryState } from "@/ipc/history";
import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

/**
 * SPEC: P3-ANN-011 — flatten every appearance-bearing annotation into the page
 * content streams on the Rust document actor. Undoable in-session (the inverse is
 * a pre-flatten snapshot); permanent once the file is saved + reopened. Returns
 * the new undo/redo availability; the caller bumps the render epoch so the canvas
 * re-renders the baked content and the sidebar re-reads.
 */
export async function flattenAnnotations(id: DocumentId): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_flatten_annotations", { id });
}
