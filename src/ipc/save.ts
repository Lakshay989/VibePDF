import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

/**
 * Reply payload of {@link savePdf}. Mirrors `pdf::document::SaveOutcome`
 * on the Rust side.
 *
 * `noOp` is `true` only when a same-path save found no unsaved changes
 * and left the file untouched (`bytesWritten === 0`).
 */
export interface SaveOutcome {
  path: string;
  bytesWritten: number;
  noOp: boolean;
}

/**
 * SPEC: P2-SAVE-001 — explicit save. Omit `path` to save the document to
 * its own path; pass a path for save-as. The write (atomic temp+rename,
 * `.bak` rotation, round-trip verification) runs on the Rust document
 * actor — the frontend never touches PDF bytes.
 */
export async function savePdf(
  id: DocumentId,
  path?: string,
): Promise<SaveOutcome> {
  return invoke<SaveOutcome>("pdf_save", { id, path: path ?? null });
}
