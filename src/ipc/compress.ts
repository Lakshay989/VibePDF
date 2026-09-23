import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

/** SPEC: P7-OCR-010 — the three levels. Mirrors `pdf::compress::CompressLevel`. */
export type CompressLevel = "low" | "medium" | "high";

/** What a compression run did. Mirrors `pdf::compress::CompressReport`. */
export interface CompressReport {
  beforeBytes: number;
  afterBytes: number;
  imagesRecompressed: number;
  imagesUntouched: number;
  streamsDeflated: number;
}

/**
 * SPEC: P7-OCR-010 — write a smaller copy of the document to `path`.
 *
 * The open document is not modified. Image recompression is lossy, so this
 * produces a new file rather than an undoable edit — which also means the
 * original is always still there if the result is not good enough.
 */
export async function compressDocument(
  id: DocumentId,
  path: string,
  level: CompressLevel,
): Promise<CompressReport> {
  return invoke<CompressReport>("pdf_compress_document", { id, path, level });
}
