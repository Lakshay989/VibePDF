// SPEC: P6-SEC-003 (P6.A4) — the pixel half of importing a signature image.
//
// Everything here is pure: given an RGBA buffer, which pixels are background,
// and where is the ink. That is the same split A2 made between `draw.ts`
// (decisions) and `raster.ts` (canvas calls), and it exists for the same
// reason — jsdom has no canvas, so anything that needs a real 2D context
// cannot be unit-tested, while a `Uint8ClampedArray` can be built by hand.
//
// Why this runs in the frontend at all: the Rust side deliberately cannot
// decode JPEG or BMP. `Cargo.toml` takes `png` alone rather than the `image`
// crate — "we don't want decoders or manipulation routines we'll never call" —
// and `pdf/image_xobject.rs` embeds JPEG verbatim as `/DCTDecode` without ever
// looking at its pixels. Thresholding in Rust would mean adding a decoder set
// to duplicate one the WebView already ships.
//
// Transparency is only ever removed here, never granted: alpha goes to 0 or
// stays where it was. A PNG that arrives with a transparent background comes
// out with the same transparent background.

import type { Box } from "@/tools/signature/draw";

/**
 * The threshold at which nothing is removed. One past pure white, so that even
 * a 255-luminance pixel survives — "off" has to mean genuinely untouched, or
 * importing an already-transparent PNG would quietly rewrite it.
 */
export const THRESHOLD_OFF = 256;

/**
 * Perceived brightness, 0–255, by the Rec. 601 weights the rest of the imaging
 * world uses. Not a plain mean: the eye is roughly twice as sensitive to green
 * as to red and five times as sensitive as to blue, so a saturated blue reads
 * far darker than a grey of the same numeric average. Ink photographed under a
 * colour cast depends on that distinction.
 */
export function luminance(r: number, g: number, b: number): number {
  return 0.299 * r + 0.587 * g + 0.114 * b;
}

/**
 * Map a 0–100 slider position to a luminance cutoff.
 *
 * The slider reads as *strength*, because that is the direction a user expects
 * to drag — more to the right, more paper gone. The cutoff runs the other way:
 * 0 strength is `THRESHOLD_OFF` (nothing touched) and 100 is 0 (everything
 * goes, which the caller rejects rather than storing a blank signature).
 */
export function strengthToThreshold(strength: number): number {
  const clamped = Math.min(Math.max(strength, 0), 100);
  return THRESHOLD_OFF - Math.round((clamped * THRESHOLD_OFF) / 100);
}

/**
 * Erase background in place: every pixel at or above `threshold` luminance has
 * its alpha zeroed. Returns how many pixels that removed.
 *
 * Only alpha is written — the colour channels are left alone, so nothing is
 * destroyed that a lower threshold could not bring back. Pixels that were
 * already transparent are skipped, so the count means "removed by this call"
 * and not "transparent afterwards".
 *
 * This is the spec's "simple threshold", and simple is the whole of it: one
 * global cutoff has no answer for a photo lit unevenly across the page. The
 * live preview and the slider are what make that workable — the user finds a
 * value that suits their image rather than the algorithm guessing.
 */
export function applyThreshold(rgba: Uint8ClampedArray, threshold: number): number {
  if (threshold >= THRESHOLD_OFF) return 0;

  let erased = 0;
  for (let i = 0; i < rgba.length; i += 4) {
    if (rgba[i + 3] === 0) continue;
    if (luminance(rgba[i] ?? 0, rgba[i + 1] ?? 0, rgba[i + 2] ?? 0) >= threshold) {
      rgba[i + 3] = 0;
      erased += 1;
    }
  }
  return erased;
}

/** How many pixels are transparent. Zero means the image will be placed as a
 *  solid rectangle — the JPEG case, which the UI warns about. */
export function countTransparent(rgba: Uint8ClampedArray, minAlpha = 0): number {
  let count = 0;
  for (let i = 3; i < rgba.length; i += 4) {
    if ((rgba[i] ?? 0) <= minAlpha) count += 1;
  }
  return count;
}

/**
 * The box enclosing every pixel more opaque than `minAlpha`, or `null` when
 * nothing is left. This is what crops the stored signature down to the ink.
 *
 * Extents are **inclusive** — one opaque pixel gives a 1×1 box, not a 0×0 one.
 * That differs from `strokeBounds`, deliberately: a captured point is a
 * position and has no area, a pixel is an area. Getting this wrong shaves a
 * row and a column off every imported signature.
 */
export function opaqueBounds(
  rgba: Uint8ClampedArray,
  width: number,
  height: number,
  minAlpha = 0,
): Box | null {
  let minX = width;
  let minY = height;
  let maxX = -1;
  let maxY = -1;

  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      if ((rgba[(y * width + x) * 4 + 3] ?? 0) <= minAlpha) continue;
      if (x < minX) minX = x;
      if (x > maxX) maxX = x;
      if (y < minY) minY = y;
      if (y > maxY) maxY = y;
    }
  }

  if (maxX < 0) return null;
  return { x: minX, y: minY, width: maxX - minX + 1, height: maxY - minY + 1 };
}
