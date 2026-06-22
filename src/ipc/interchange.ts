import type { HistoryState } from "@/ipc/history";
import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

/**
 * SPEC: P3-ANN-010 — export every annotation in the document to an XFDF file at
 * `path`. The serialize + write runs on the Rust document actor; the frontend
 * never touches PDF bytes. Returns the count of annotations written.
 */
export async function exportAnnotations(id: DocumentId, path: string): Promise<number> {
  return invoke<number>("pdf_export_annotations", { id, path });
}

/**
 * SPEC: P3-ANN-010 — import the annotations described by the XFDF file at `path`,
 * applied as one undoable edit. Returns the new undo/redo availability; the
 * caller re-reads the annotation list (and bumps the render epoch) to reflect
 * the recreated annotations.
 */
export async function importAnnotations(id: DocumentId, path: string): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_import_annotations", { id, path });
}
