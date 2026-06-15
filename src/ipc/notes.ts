import type { HistoryState } from "@/ipc/history";
import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

/**
 * SPEC: P3-ANN-002 — place a sticky note (`/Text` annotation) at `(x, y)` in
 * PDF points on `page` (0-based). `noteId` is a stable id we generate up front
 * and store as `/NM`, so later edits/deletes can find this exact annotation.
 * The write runs on the Rust document actor (lopdf) — the frontend never
 * touches PDF bytes. Returns the new undo/redo availability.
 */
export async function addTextNote(
  id: DocumentId,
  noteId: string,
  page: number,
  x: number,
  y: number,
  content: string,
  author: string,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_add_text_note", {
    id,
    noteId,
    page,
    x,
    y,
    content,
    author,
  });
}

/**
 * SPEC: P3-ANN-002 — replace a note's body (`/Contents`), found by its stable
 * `/NM == noteId`. Undoable. Runs on the Rust document actor.
 */
export async function updateTextNote(
  id: DocumentId,
  noteId: string,
  content: string,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_update_text_note", { id, noteId, content });
}

/**
 * SPEC: P3-ANN-002 — delete the annotation with `/NM == noteId` (used for
 * sticky notes). Undoable. Runs on the Rust document actor.
 */
export async function deleteAnnotation(
  id: DocumentId,
  noteId: string,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_delete_annotation", { id, noteId });
}

/** A sticky note read back out of the PDF. Mirrors `cos::NoteData` (Rust). */
export interface NoteData {
  /** The note's `/NM` (stable id); a synthesized `obj-<n>-<g>` if it had none. */
  nm: string;
  /** 0-based page index. */
  page: number;
  /** `/Rect` lower-left, in PDF points. */
  x: number;
  y: number;
  content: string;
  author: string;
}

/**
 * SPEC: P3-ANN-002 (re-openable) — read every sticky note out of the open
 * document. Read-only (no undo entry); used to project the PDF's notes into the
 * overlay store on open and after undo/redo. Runs on the Rust document actor.
 */
export async function readTextNotes(id: DocumentId): Promise<NoteData[]> {
  return invoke<NoteData[]>("pdf_read_text_notes", { id });
}
