import { type HistoryState } from "@/ipc/history";
import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

export interface BatesArgs {
  /** "header" (top margin) or "footer" (bottom margin). */
  position: "header" | "footer";
  align: "left" | "center" | "right";
  /** Text before the number, e.g. "ABC". */
  prefix: string;
  /** Text after the number, e.g. ".TIFF". */
  suffix: string;
  /** Minimum digit width, zero-filled (6 → 000001). */
  padding: number;
  /** The number on the first page (≥ 0). */
  start: number;
  fontFamily: string;
  fontSize: number;
  color: string;
  /** Points from the page edge. */
  margin: number;
}

/**
 * SPEC: P4-EDIT-012 (P4.D5) — stamp a gap-free Bates id on every page. Returns
 * the new undo/redo availability.
 */
export async function addBates(id: DocumentId, args: BatesArgs): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_add_bates", { id, ...args });
}
