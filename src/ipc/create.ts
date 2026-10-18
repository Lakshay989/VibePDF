import { invoke } from "@/ipc/invoke";

/** The page images and text go on. Mirrors `pdf::create::PageSize`. */
export type CreatePageSize = "a4" | "letter" | "image";

/** What a creation produced. Mirrors `pdf::create::CreateSummary`. */
export interface CreateSummary {
  pages: number;
  images: number;
  textFiles: number;
  /** Word documents. */
  documents: number;
  bytes: number;
  /** Things the person should know, e.g. a text file read as Windows-1252. */
  warnings: string[];
}

/**
 * SPEC: P7-OCR-009 (P7.C1a, C1b1) — create a new PDF at `dest` from Word
 * documents, images (JPEG, PNG, TIFF, WebP) and plain-text files, in the order
 * given. Standalone, like merge: it reads files from disk and needs no open
 * document. Fonts are embedded; JPEGs go in unchanged. What a conversion
 * simplified (a missing font, a table, tracked changes) comes back as
 * `warnings`.
 */
export async function createPdfFromFiles(
  sources: string[],
  dest: string,
  pageSize: CreatePageSize,
): Promise<CreateSummary> {
  return invoke<CreateSummary>("pdf_create_from_files", { sources, dest, pageSize });
}
