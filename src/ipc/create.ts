import { invoke } from "@/ipc/invoke";

/** The page images and text go on. Mirrors `pdf::create::PageSize`. */
export type CreatePageSize = "a4" | "letter" | "image";

/** What a creation produced. Mirrors `pdf::create::CreateSummary`. */
export interface CreateSummary {
  pages: number;
  images: number;
  textFiles: number;
  bytes: number;
  /** Things the person should know, e.g. a text file read as Windows-1252. */
  warnings: string[];
}

/**
 * SPEC: P7-OCR-009 (P7.C1a) — create a new PDF at `dest` from image (JPEG, PNG,
 * TIFF, WebP) and plain-text files, in the order given. Standalone, like
 * merge: it reads files from disk and needs no open document. Fonts are
 * embedded; JPEGs go in unchanged.
 */
export async function createPdfFromFiles(
  sources: string[],
  dest: string,
  pageSize: CreatePageSize,
): Promise<CreateSummary> {
  return invoke<CreateSummary>("pdf_create_from_files", { sources, dest, pageSize });
}
