// SPEC: P6-SEC-003 (P6.A4) — background removal, on hand-built pixels.
//
// This is where the real coverage for image import lives. A2 and A3 could only
// assert *drawing commands*, because a canvas is needed to turn them into
// pixels and jsdom has none. Thresholding has no such excuse: it is a loop over
// an RGBA buffer, and a `Uint8ClampedArray` is as real here as it is in the
// app. Every claim below is about actual pixel values.

import { describe, expect, it } from "vitest";

import {
  applyThreshold,
  countTransparent,
  luminance,
  opaqueBounds,
  strengthToThreshold,
  THRESHOLD_OFF,
} from "@/tools/signature/threshold";

/** Flat RGBA from a list of pixels. */
const px = (...pixels: Array<[number, number, number, number]>) =>
  Uint8ClampedArray.from(pixels.flat());

/** A `width`×`height` transparent buffer with the listed pixels set opaque. */
function grid(
  width: number,
  height: number,
  opaque: Array<[number, number]>,
  alpha = 255,
): Uint8ClampedArray {
  const data = new Uint8ClampedArray(width * height * 4);
  for (const [x, y] of opaque) data[(y * width + x) * 4 + 3] = alpha;
  return data;
}

describe("luminance", () => {
  it("weights green over red over blue, rather than averaging", () => {
    // Identical numeric average (85), very different brightness. A plain mean
    // would call these the same pixel, and a signature photographed under a
    // colour cast is exactly where that goes wrong.
    expect(luminance(0, 0, 255)).toBeLessThan(luminance(85, 85, 85));
    expect(luminance(0, 255, 0)).toBeGreaterThan(luminance(255, 0, 0));
  });

  it("spans the full range at the extremes", () => {
    expect(luminance(0, 0, 0)).toBe(0);
    expect(luminance(255, 255, 255)).toBeCloseTo(255, 6);
  });
});

describe("strengthToThreshold", () => {
  it("maps a slider at rest to no removal at all", () => {
    expect(strengthToThreshold(0)).toBe(THRESHOLD_OFF);
  });

  it("maps a slider at the top to removing everything", () => {
    expect(strengthToThreshold(100)).toBe(0);
  });

  it("never rises as the slider does", () => {
    let previous = Infinity;
    for (let s = 0; s <= 100; s++) {
      const t = strengthToThreshold(s);
      expect(t).toBeLessThanOrEqual(previous);
      previous = t;
    }
  });

  it("clamps rather than extrapolating past the ends", () => {
    expect(strengthToThreshold(-40)).toBe(THRESHOLD_OFF);
    expect(strengthToThreshold(400)).toBe(0);
  });
});

describe("applyThreshold", () => {
  it("erases pixels brighter than the cutoff and keeps darker ones", () => {
    const data = px([250, 250, 250, 255], [20, 20, 20, 255]);
    expect(applyThreshold(data, 128)).toBe(1);
    expect(data[3]).toBe(0); // the near-white one is gone
    expect(data[7]).toBe(255); // the ink survived
  });

  it("treats the cutoff itself as background", () => {
    // Pinning `>=` rather than `>`, without depending on float rounding: the
    // cutoff is the pixel's own measured luminance.
    const cut = luminance(120, 130, 140);
    expect(applyThreshold(px([120, 130, 140, 255]), cut)).toBe(1);
    expect(applyThreshold(px([120, 130, 140, 255]), cut + 0.001)).toBe(0);
  });

  it("judges by luminance, not by the average of the channels", () => {
    // Both average 85. At a cutoff of 60 the grey is background and the
    // saturated blue is ink.
    const data = px([85, 85, 85, 255], [0, 0, 255, 255]);
    expect(applyThreshold(data, 60)).toBe(1);
    expect(data[3]).toBe(0);
    expect(data[7]).toBe(255);
  });

  it("leaves the colour channels alone", () => {
    // Only alpha is written, so a too-aggressive slider is always recoverable
    // by dragging back — nothing about the pixel has actually been destroyed.
    const data = px([250, 240, 230, 255]);
    applyThreshold(data, 100);
    expect(Array.from(data.subarray(0, 3))).toEqual([250, 240, 230]);
  });

  it("never resurrects transparency it did not create", () => {
    // A PNG that arrived with a transparent background keeps it, whatever the
    // colour channels happen to hold underneath.
    const data = px([0, 0, 0, 0]);
    expect(applyThreshold(data, 200)).toBe(0);
    expect(data[3]).toBe(0);
  });

  it("does nothing at all when removal is off", () => {
    const data = px([255, 255, 255, 255]);
    expect(applyThreshold(data, THRESHOLD_OFF)).toBe(0);
    // Even pure white survives — "off" has to mean untouched, or importing an
    // already-clean PNG would quietly rewrite it.
    expect(data[3]).toBe(255);
  });

  it("erases everything at full strength", () => {
    const data = px([0, 0, 0, 255], [255, 255, 255, 255]);
    expect(applyThreshold(data, strengthToThreshold(100))).toBe(2);
  });

  it("counts what it removed, not what ends up transparent", () => {
    const data = px([255, 255, 255, 255], [255, 255, 255, 0], [10, 10, 10, 255]);
    expect(applyThreshold(data, 128)).toBe(1);
    expect(countTransparent(data)).toBe(2);
  });
});

describe("countTransparent", () => {
  it("counts fully transparent pixels", () => {
    expect(countTransparent(px([0, 0, 0, 0], [0, 0, 0, 255], [0, 0, 0, 0]))).toBe(2);
  });

  it("returns zero for an image with no alpha — the JPEG case the UI warns about", () => {
    expect(countTransparent(px([1, 2, 3, 255], [4, 5, 6, 255]))).toBe(0);
  });

  it("honours a minimum alpha", () => {
    expect(countTransparent(px([0, 0, 0, 10]), 10)).toBe(1);
    expect(countTransparent(px([0, 0, 0, 10]), 9)).toBe(0);
  });
});

describe("opaqueBounds", () => {
  it("spans only the pixels that are left", () => {
    const box = opaqueBounds(grid(4, 3, [[1, 1], [2, 2]]), 4, 3);
    expect(box).toEqual({ x: 1, y: 1, width: 2, height: 2 });
  });

  it("uses inclusive extents, so one pixel is 1×1 and not 0×0", () => {
    // The difference from `strokeBounds` is deliberate: a captured point is a
    // position, a pixel is an area. Treating them alike shaves a row and a
    // column off every imported signature.
    expect(opaqueBounds(grid(4, 3, [[2, 0]]), 4, 3)).toEqual({
      x: 2,
      y: 0,
      width: 1,
      height: 1,
    });
  });

  it("covers the whole frame when nothing was removed", () => {
    const full: Array<[number, number]> = [];
    for (let y = 0; y < 3; y++) for (let x = 0; x < 4; x++) full.push([x, y]);
    expect(opaqueBounds(grid(4, 3, full), 4, 3)).toEqual({ x: 0, y: 0, width: 4, height: 3 });
  });

  it("returns null when the threshold took everything", () => {
    expect(opaqueBounds(grid(4, 3, []), 4, 3)).toBeNull();
  });

  it("ignores pixels at or below the minimum alpha", () => {
    const faint = grid(4, 3, [[1, 1]], 10);
    expect(opaqueBounds(faint, 4, 3, 10)).toBeNull();
    expect(opaqueBounds(faint, 4, 3, 9)).toEqual({ x: 1, y: 1, width: 1, height: 1 });
  });

  it("reads rows and columns the right way round", () => {
    // A transposed index would pass every square-image test ever written.
    const box = opaqueBounds(grid(5, 2, [[4, 1]]), 5, 2);
    expect(box).toEqual({ x: 4, y: 1, width: 1, height: 1 });
  });
});
