import type { HistoryState } from "@/ipc/history";
import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

/**
 * SPEC: P3-ANN-009 — reply to the annotation with handle `parentId` (the same
 * `/NM` / `obj:` id the sidebar uses for select/delete). Persists a `/Text`
 * linked via `/IRT` on the Rust document actor — the frontend never touches PDF
 * bytes. Returns the new undo/redo availability.
 */
export async function addReply(
  id: DocumentId,
  parentId: string,
  author: string,
  content: string,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_add_reply", { id, parentId, author, content });
}
