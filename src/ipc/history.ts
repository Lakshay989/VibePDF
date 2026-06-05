import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

/**
 * Undo/redo availability for a document. Mirrors
 * `pdf::undo::HistoryState` on the Rust side. The frontend stores this
 * per document to drive the Undo/Redo button state; the actual stacks
 * live in the document actor (the frontend never edits PDF bytes).
 */
export interface HistoryState {
  canUndo: boolean;
  canRedo: boolean;
}

/** SPEC: P2-PAGE-003 / session history — undo the most recent edit. */
export async function undo(id: DocumentId): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_undo", { id });
}

/** Redo the most recently undone edit. */
export async function redo(id: DocumentId): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_redo", { id });
}

/** Query current availability (used to hydrate the store on open). */
export async function historyState(id: DocumentId): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_history_state", { id });
}
