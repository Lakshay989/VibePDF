import { invoke } from "@/ipc/invoke";
import type { SaveOutcome } from "@/ipc/save";

/**
 * SPEC: P2-PAGE-008 (partial) — merge `paths` (≥ 2, in order) into a new PDF
 * at `dest`. Standalone: operates on files on disk, not an open document, so
 * there's no `DocumentId`. Concatenates pages + page annotations; bookmarks
 * and form fields are not yet carried (deferred to the lopdf follow-up). The
 * write runs on the Rust side — the frontend never touches PDF bytes.
 */
export async function mergeDocuments(
  paths: string[],
  dest: string,
): Promise<SaveOutcome> {
  return invoke<SaveOutcome>("pdf_merge_documents", { paths, dest });
}
