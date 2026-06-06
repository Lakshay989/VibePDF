import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";
import type { SaveOutcome } from "@/ipc/save";

/**
 * How to divide a PDF. Mirrors `pdf::split::SplitMode` on the Rust side
 * (serde tag `kind`). SPEC: P2-PAGE-007.
 *
 * - `everyN` — every `n` pages becomes one file.
 * - `atPages` — split *before* each 1-based page number in `boundaries`.
 * - `bySize` — grow a file until adding the next page would exceed `maxBytes`.
 * - `bookmarks` — start a new file at every top-level bookmark's page.
 */
export type SplitMode =
  | { kind: "everyN"; n: number }
  | { kind: "atPages"; boundaries: number[] }
  | { kind: "bySize"; maxBytes: number }
  | { kind: "bookmarks" };

/** Reply payload of {@link splitDocument} — one outcome per output file. */
export interface SplitOutcome {
  files: SaveOutcome[];
}

/**
 * SPEC: P2-PAGE-007 — split the document into multiple PDFs under `destDir`,
 * named `{stem}-NNN.pdf`. Read-only on the source: the open document is
 * unchanged. The write runs on the Rust document actor — the frontend never
 * touches PDF bytes.
 */
export async function splitDocument(
  id: DocumentId,
  mode: SplitMode,
  destDir: string,
  stem: string,
): Promise<SplitOutcome> {
  return invoke<SplitOutcome>("pdf_split_document", {
    id,
    mode,
    destDir,
    stem,
  });
}
