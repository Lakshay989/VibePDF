// Read a local image file into a `data:` URL for an <img> preview (P4.HF29).
//
// The optimistic-edit overlays show a just-placed image before the backend
// bakes it into the page. The app CSP allows `img-src data:`, and a data URL is
// self-contained (no object-URL lifecycle to revoke — it's freed with the held
// edit). Image reads are local and fast, so this doesn't gate the placement.

import { readFile } from "@tauri-apps/plugin-fs";

/** Best-effort image MIME from a file extension (backend accepts PNG/JPEG). */
function imageMime(path: string): string {
  const ext = path.toLowerCase().split(".").pop() ?? "";
  switch (ext) {
    case "png":
      return "image/png";
    case "jpg":
    case "jpeg":
      return "image/jpeg";
    case "gif":
      return "image/gif";
    case "webp":
      return "image/webp";
    case "bmp":
      return "image/bmp";
    default:
      return "application/octet-stream";
  }
}

/**
 * Encode raw bytes as a `data:` URL. Used where the bytes came from somewhere
 * other than a file path — a signature fetched from the library (P6.A4/A5a),
 * or one just rasterised in memory.
 *
 * Chunked because `String.fromCharCode(...bytes)` spreads every byte into the
 * argument list, which overflows the stack well below the size of a signature
 * PNG.
 */
export function bytesToDataUrl(bytes: Uint8Array, mime = "image/png"): string {
  const CHUNK = 0x8000;
  let binary = "";
  for (let i = 0; i < bytes.length; i += CHUNK) {
    binary += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  return `data:${mime};base64,${btoa(binary)}`;
}

/** Read `path` and encode it as a `data:` URL suitable for an <img> `src`. */
export async function fileToDataUrl(path: string): Promise<string> {
  const bytes = await readFile(path);
  const blob = new Blob([bytes], { type: imageMime(path) });
  return await new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result as string);
    reader.onerror = () => reject(reader.error ?? new Error("failed to read image"));
    reader.readAsDataURL(blob);
  });
}

/** Decode an image `src` and return its aspect ratio (width / height, ≥ 0). */
export async function imageAspect(src: string): Promise<number> {
  return await new Promise<number>((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve(img.naturalHeight > 0 ? img.naturalWidth / img.naturalHeight : 1);
    img.onerror = () => reject(new Error("failed to decode image"));
    img.src = src;
  });
}
