import type { HistoryState } from "@/ipc/history";
import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";
import type { MarkupSubtype, Quad } from "@/tools/_framework";

/**
 * SPEC: P3-ANN-001 — add a text-markup annotation over `quads` (each
 * `[x1..y4]` in PDF points) on `page` (0-based). The write runs on the Rust
 * document actor (lopdf, with a generated `/AP` appearance) — the frontend
 * never touches PDF bytes. Returns the new undo/redo availability.
 */
export async function addTextMarkup(
  id: DocumentId,
  page: number,
  subtype: MarkupSubtype,
  quads: Quad[],
  color: string,
  opacity: number,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_add_text_markup", {
    id,
    page,
    subtype,
    quads,
    color,
    opacity,
  });
}
