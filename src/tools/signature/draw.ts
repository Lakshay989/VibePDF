// SPEC: P6-SEC-001 (P6.A2) — the geometry half of drawing a signature.
//
// Everything here is pure: bounds of the captured ink, and the transform that
// maps it into the raster we store. That split is deliberate — the actual
// canvas calls live in `raster.ts`, which cannot be unit-tested (jsdom has no
// canvas implementation, and `canvas` is a native dependency this project does
// not carry). Keeping every *decision* on this side means the untestable part
// is a dozen lines of drawing with nothing to get wrong.
//
// Smoothing is not reimplemented here: `tools/ink/ink.ts::smoothInk` already
// does simplify + Catmull-Rom resample for P3-ANN-005, which is exactly what
// the spec's "with smoothing" asks for.

import type { InkPoint } from "@/tools/ink/ink";

/** One continuous pen-down..pen-up gesture, in pad-local CSS pixels. */
export type Stroke = InkPoint[];

/** An axis-aligned box. `width`/`height` may be 0 for a dot or a straight line. */
export interface Box {
  x: number;
  y: number;
  width: number;
  height: number;
}

/**
 * How to draw the ink into the stored raster: the output size, and the
 * scale/offset that maps a captured point into it.
 */
export interface RasterFit {
  /** Output canvas size in device pixels. */
  width: number;
  height: number;
  /** Multiply a captured coordinate by this… */
  scale: number;
  /** …then add these, to land in raster space. */
  offsetX: number;
  offsetY: number;
}

/** Long edge of the stored PNG, in pixels. Fixed so a signature drawn on a
 *  small pad still looks sharp when placed large later (P6.A5). */
export const TARGET_LONG_EDGE = 600;

/** Transparent margin around the ink, in raster pixels — keeps round caps and
 *  the outer half of the stroke width from being clipped at the edge. */
export const RASTER_PAD = 12;

/**
 * The bounding box of every point across every stroke, or `null` when there is
 * no ink at all. Empty strokes are skipped rather than contributing a spurious
 * point at the origin.
 */
export function strokeBounds(strokes: readonly Stroke[]): Box | null {
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  let seen = false;

  for (const stroke of strokes) {
    for (const p of stroke) {
      seen = true;
      if (p.x < minX) minX = p.x;
      if (p.y < minY) minY = p.y;
      if (p.x > maxX) maxX = p.x;
      if (p.y > maxY) maxY = p.y;
    }
  }
  if (!seen) return null;
  return { x: minX, y: minY, width: maxX - minX, height: maxY - minY };
}

/**
 * Fit `bounds` into a raster whose long edge is `target`, with `pad` pixels of
 * transparent margin on every side. The aspect ratio is preserved; the ink is
 * trimmed to its own extent, so the stored PNG is the signature and not the
 * pad it was drawn on.
 *
 * Degenerate ink is handled rather than guarded against by the caller: a single
 * dot (zero width *and* height) has no meaningful scale, so it renders at 1:1;
 * a perfectly straight line (one dimension zero) still scales by its other one.
 */
export function fitToRaster(
  bounds: Box,
  target: number = TARGET_LONG_EDGE,
  pad: number = RASTER_PAD,
): RasterFit {
  const longest = Math.max(bounds.width, bounds.height);
  // A dot has nothing to scale — blowing it up to 600px would just be a huge
  // blob, and `inner / 0` is Infinity.
  const scale = longest > 0 ? Math.max(target - 2 * pad, 1) / longest : 1;

  return {
    width: Math.max(Math.round(bounds.width * scale + 2 * pad), 2 * pad),
    height: Math.max(Math.round(bounds.height * scale + 2 * pad), 2 * pad),
    scale,
    offsetX: pad - bounds.x * scale,
    offsetY: pad - bounds.y * scale,
  };
}

/** Map a captured point into raster space under `fit`. */
export function project(p: InkPoint, fit: RasterFit): { x: number; y: number } {
  return { x: p.x * fit.scale + fit.offsetX, y: p.y * fit.scale + fit.offsetY };
}

/** Whether there is anything worth saving. A stroke of one point counts — a
 *  deliberate dot is a mark; an empty stroke list is not. */
export function hasInk(strokes: readonly Stroke[]): boolean {
  return strokes.some((s) => s.length > 0);
}
