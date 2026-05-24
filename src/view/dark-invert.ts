// SPEC: P1-VIEW-010 (P1.C5) — dark-mode page invert.
//
// Goal: page background flips from white → near-black, text from
// black → white, but photos still look like photos.
//
// Strategy: a luminance invert that ONLY touches near-grayscale
// pixels. A pixel's "grayscale-ness" is (max(R,G,B) − min(R,G,B))
// / max(R,G,B) — the HSV saturation. Below the threshold, we
// invert each channel; above, we leave it alone. This is a
// heuristic, not a parse of the PDF operator list (which the
// spec hints at), but it covers black-on-white text, line art,
// and colored photos correctly without needing to know which
// regions came from `Image` operators.
//
// Known limitations:
//  - Colored text (e.g. red headings) won't invert — saturation
//    is high, so it stays red. In dark mode it'll look red on
//    near-black, which is usually still readable.
//  - Photos that are mostly grayscale (e.g. an X-ray scan) will
//    get inverted as if they were text. The "real" fix is the
//    operator-list approach; a future Phase 4-era refinement.

const GRAYSCALE_SATURATION_THRESHOLD = 0.15;

/**
 * In-place luminance invert of an RGBA pixel buffer, applied only
 * to near-grayscale pixels. Alpha is untouched.
 *
 * Exported separately from the canvas wrapper so it can be unit-
 * tested without a real canvas.
 */
export function invertImageDataForDark(data: Uint8ClampedArray): void {
  for (let i = 0; i < data.length; i += 4) {
    const r = data[i];
    const g = data[i + 1];
    const b = data[i + 2];
    const max = Math.max(r, g, b);
    const min = Math.min(r, g, b);
    // saturation in [0, 1]. max === 0 (pure black) → treat as grayscale.
    const saturation = max === 0 ? 0 : (max - min) / max;
    if (saturation < GRAYSCALE_SATURATION_THRESHOLD) {
      data[i] = 255 - r;
      data[i + 1] = 255 - g;
      data[i + 2] = 255 - b;
    }
  }
}

/**
 * Apply the dark-mode invert to the given canvas. Reads back through
 * `getImageData` / `putImageData`, so the canvas must be readable
 * (CORS-tainted canvases will throw). Our PDF.js-rendered canvases
 * are same-origin, so this is safe here.
 */
export function invertCanvasForDarkMode(canvas: HTMLCanvasElement): void {
  if (canvas.width === 0 || canvas.height === 0) return;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  const img = ctx.getImageData(0, 0, canvas.width, canvas.height);
  invertImageDataForDark(img.data);
  ctx.putImageData(img, 0, 0);
}
