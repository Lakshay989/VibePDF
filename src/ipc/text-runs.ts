import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

/**
 * One extracted text run, mirroring `text_extract::TextRun` (Rust). Geometry is
 * in PDF points (page space, origin bottom-left) — the same space annotations
 * use, so a click can be hit-tested against `bbox` after `screenToPdf`.
 */
export interface TextRun {
  /** The run's Unicode text. */
  text: string;
  /** Axis-aligned bounds `[x0, y0, x1, y1]` (over-covers rotated text). */
  bbox: [number, number, number, number];
  /** Font name, subset tag (`ABCDEF+`) stripped. */
  fontName: string;
  /** Whether the font is embedded in the file (drives the P4.A2 fallback). */
  embedded: boolean;
  /** Scaled (on-screen) font size in points. */
  fontSize: number;
  /** Fill colour `#rrggbb`. */
  color: string;
  /** The text matrix `[a, b, c, d, e, f]`. */
  transform: [number, number, number, number, number, number];
}

/**
 * SPEC: P4-EDIT-001 (P4.A1) — extract every text run on `page` (0-based) of the
 * open document, for click-to-edit hit-testing (P4.B1). Read-only; runs on the
 * Rust document actor against the live PDFium document — the frontend never
 * touches PDF bytes.
 */
export async function extractTextRuns(id: DocumentId, page: number): Promise<TextRun[]> {
  return invoke<TextRun[]>("pdf_extract_text_runs", { id, page });
}
