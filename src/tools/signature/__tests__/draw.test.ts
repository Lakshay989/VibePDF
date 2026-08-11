// SPEC: P6-SEC-001 (P6.A2) — the pure geometry behind a drawn signature.
//
// These carry the weight for this step: `raster.ts` cannot be unit-tested
// (no canvas under jsdom), so every decision it relies on is made here and
// asserted here.

import { describe, expect, it } from "vitest";

import {
  fitToRaster,
  hasInk,
  project,
  RASTER_PAD,
  strokeBounds,
  TARGET_LONG_EDGE,
  type Stroke,
} from "@/tools/signature/draw";

const pt = (x: number, y: number) => ({ x, y, pressure: 0.5 });

describe("strokeBounds", () => {
  it("spans every point across every stroke", () => {
    const strokes: Stroke[] = [
      [pt(10, 10), pt(20, 30)],
      [pt(5, 25), pt(40, 12)],
    ];
    expect(strokeBounds(strokes)).toEqual({ x: 5, y: 10, width: 35, height: 20 });
  });

  it("is null when there is no ink", () => {
    expect(strokeBounds([])).toBeNull();
    // An empty stroke must not contribute a phantom point at the origin.
    expect(strokeBounds([[], []])).toBeNull();
  });

  it("gives a single point a zero-area box", () => {
    expect(strokeBounds([[pt(7, 9)]])).toEqual({ x: 7, y: 9, width: 0, height: 0 });
  });
});

describe("fitToRaster", () => {
  it("scales the long edge to the target, padding included", () => {
    const fit = fitToRaster({ x: 0, y: 0, width: 300, height: 100 });
    expect(fit.width).toBe(TARGET_LONG_EDGE);
    // Aspect preserved: a 3:1 box stays 3:1 across the *ink*, not the padding.
    expect(fit.height).toBe(Math.round(100 * fit.scale) + 2 * RASTER_PAD);
  });

  it("preserves aspect ratio when the tall edge is the long one", () => {
    const fit = fitToRaster({ x: 0, y: 0, width: 50, height: 400 });
    expect(fit.height).toBe(TARGET_LONG_EDGE);
    expect(fit.width).toBeLessThan(fit.height);
  });

  it("pads without clipping — the ink lands inside the raster", () => {
    const bounds = { x: 100, y: 40, width: 200, height: 80 };
    const fit = fitToRaster(bounds);

    const topLeft = project(pt(bounds.x, bounds.y), fit);
    const bottomRight = project(pt(bounds.x + bounds.width, bounds.y + bounds.height), fit);
    expect(topLeft.x).toBeCloseTo(RASTER_PAD);
    expect(topLeft.y).toBeCloseTo(RASTER_PAD);
    // The canvas size is rounded to whole pixels, so the far edge can sit up to
    // half a pixel inside or outside the nominal padding. Asserting exactness
    // here would be asserting that canvases can be fractional.
    expect(Math.abs(bottomRight.x - (fit.width - RASTER_PAD))).toBeLessThanOrEqual(0.5);
    expect(Math.abs(bottomRight.y - (fit.height - RASTER_PAD))).toBeLessThanOrEqual(0.5);
  });

  it("trims to the ink, so the pad it was drawn on is not stored", () => {
    // Same signature, drawn in the corner of a big pad vs at the origin.
    const a = fitToRaster({ x: 0, y: 0, width: 120, height: 40 });
    const b = fitToRaster({ x: 900, y: 500, width: 120, height: 40 });
    expect(b.width).toBe(a.width);
    expect(b.height).toBe(a.height);
  });

  it("renders a dot at 1:1 instead of dividing by zero", () => {
    const fit = fitToRaster({ x: 3, y: 3, width: 0, height: 0 });
    expect(Number.isFinite(fit.scale)).toBe(true);
    expect(fit.scale).toBe(1);
    expect(fit.width).toBe(2 * RASTER_PAD);
    expect(fit.height).toBe(2 * RASTER_PAD);
  });

  it("scales a straight line by its non-zero dimension", () => {
    const fit = fitToRaster({ x: 0, y: 0, width: 300, height: 0 });
    expect(fit.width).toBe(TARGET_LONG_EDGE);
    // No height to scale, but the padding still gives it a drawable band.
    expect(fit.height).toBe(2 * RASTER_PAD);
  });
});

describe("hasInk", () => {
  it("is false for nothing and for empty strokes", () => {
    expect(hasInk([])).toBe(false);
    expect(hasInk([[], []])).toBe(false);
  });

  it("counts a deliberate single dot as ink", () => {
    expect(hasInk([[pt(1, 1)]])).toBe(true);
  });
});
