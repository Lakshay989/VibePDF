import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

/** The four formats P7-OCR-005 names. Mirrors `pdf::export_image::ImageExportFormat`. */
export type ImageExportFormat = "png" | "jpeg" | "tiff" | "webp";

/** What an export wrote. Mirrors `pdf::export_image::ImageExportSummary`. */
export interface ImageExportSummary {
  pages: number;
  /** File names, in the order written. The caller knows the directory. */
  files: string[];
  bytes: number;
}

/** The DPI bounds P7-OCR-005 specifies; the backend rejects anything outside. */
export const MIN_DPI = 72;
export const MAX_DPI = 600;

export interface ImageExportOptions {
  format: ImageExportFormat;
  dpi: number;
  /** JPEG only, 1–100. Ignored by the three lossless formats. */
  quality?: number;
}

/**
 * SPEC: P7-OCR-005 — render pages into `destDir` as `{stem}-{n:03}.{ext}`,
 * where `n` is the page's own 1-based number. Pass an empty `pages` for the
 * whole document.
 *
 * The PDF is not modified, so there is nothing to undo afterwards.
 */
export async function exportImages(
  id: DocumentId,
  destDir: string,
  stem: string,
  pages: number[],
  options: ImageExportOptions,
): Promise<ImageExportSummary> {
  return invoke<ImageExportSummary>("pdf_export_images", {
    id,
    destDir,
    stem,
    pages,
    format: options.format,
    dpi: options.dpi,
    quality: options.quality ?? 85,
  });
}
