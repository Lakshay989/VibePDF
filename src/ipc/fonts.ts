import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

/**
 * How a document font maps onto what we can render when editing it. Mirrors
 * `font_resolver::FontStatus` (Rust).
 * - `embedded` / `standard` / `systemAvailable` — editing is safe.
 * - `fallback` — editing substitutes `substitute` and is lossy.
 */
export type FontStatus = "embedded" | "standard" | "systemAvailable" | "fallback";

/** One document font's resolution. Mirrors `font_resolver::FontResolution`. */
export interface FontResolution {
  /** The font name as it appears in the file (subset tag stripped). */
  fontName: string;
  /** Whether the file embeds this font's glyphs. */
  embedded: boolean;
  /** The resolution outcome. */
  status: FontStatus;
  /** The base-14 face we'd substitute — non-null only when `status === "fallback"`. */
  substitute: string | null;
}

/** The document-wide font report. Mirrors `font_resolver::FontReport`. */
export interface FontReport {
  fonts: FontResolution[];
  /** True iff any font would be substituted when edited. */
  needsFallback: boolean;
}

/**
 * SPEC: P4-EDIT-002 (P4.A2) — resolve the open document's fonts against the
 * system, so the UI can warn *once per document* when an edit would substitute
 * a missing face. Read-only; runs on the Rust document actor against the live
 * PDFium document — the frontend never touches PDF bytes.
 */
export async function readFontReport(id: DocumentId): Promise<FontReport> {
  return invoke<FontReport>("pdf_read_font_report", { id });
}
