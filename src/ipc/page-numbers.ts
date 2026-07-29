import { type HistoryState } from "@/ipc/history";
import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

/** The number-rendering styles (SPEC P4-EDIT-011); must match the Rust wire strings. */
export type NumberFormat =
  | "decimal" // 1
  | "decimal-slash-total" // 1/N
  | "page-x-of-n" // Page 1 of N
  | "lower-roman" // i
  | "upper-roman" // I
  | "lower-alpha" // a
  | "upper-alpha"; // A

export interface PageNumbersArgs {
  /** "header" (top margin) or "footer" (bottom margin). */
  position: "header" | "footer";
  align: "left" | "center" | "right";
  format: NumberFormat;
  /** The number shown on the first page (≥ 1). */
  start: number;
  fontFamily: string;
  fontSize: number;
  color: string;
  /** Points from the page edge. */
  margin: number;
}

/**
 * SPEC: P4-EDIT-011 (P4.D4) — stamp a page number on every page except the
 * 0-based indices in `exclude`. Returns the new undo/redo availability.
 */
export async function addPageNumbers(
  id: DocumentId,
  exclude: number[],
  args: PageNumbersArgs,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_add_page_numbers", { id, exclude, ...args });
}
