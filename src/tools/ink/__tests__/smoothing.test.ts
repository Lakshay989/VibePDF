// SPEC: P3-ANN-005 — the freehand smoothing pipeline: simplify away jitter, then
// resample a Catmull-Rom spline through the survivors. Pure functions, no DOM.

import { describe, expect, it } from "vitest";

import { catmullRomResample, type InkPoint, simplify, smoothInk } from "@/tools/ink/ink";

const p = (x: number, y: number, pressure = 0.5): InkPoint => ({ x, y, pressure });

describe("catmullRomResample", () => {
  it("passes through the first and last input point", () => {
    const input = [p(0, 0), p(100, 0), p(100, 100)];
    const out = catmullRomResample(input, 5);
    expect(out[0].x).toBeCloseTo(0);
    expect(out[0].y).toBeCloseTo(0);
    expect(out[out.length - 1].x).toBeCloseTo(100);
    expect(out[out.length - 1].y).toBeCloseTo(100);
  });

  it("increases density on a coarse path", () => {
    const input = [p(0, 0), p(90, 0), p(90, 90)];
    const out = catmullRomResample(input, 3);
    expect(out.length).toBeGreaterThan(input.length);
  });

  it("keeps interpolated pressure within the segment's range", () => {
    const out = catmullRomResample([p(0, 0, 0.2), p(100, 0, 0.9)], 5);
    for (const s of out) {
      expect(s.pressure).toBeGreaterThanOrEqual(0.2 - 1e-6);
      expect(s.pressure).toBeLessThanOrEqual(0.9 + 1e-6);
    }
  });

  it("handles two points (a straight segment)", () => {
    const out = catmullRomResample([p(0, 0), p(50, 0)], 10);
    expect(out.length).toBeGreaterThanOrEqual(2);
    expect(out[out.length - 1].x).toBeCloseTo(50);
  });

  it("passes degenerate inputs (0 or 1 point) straight through", () => {
    expect(catmullRomResample([])).toEqual([]);
    expect(catmullRomResample([p(7, 7)])).toEqual([p(7, 7)]);
  });
});

describe("simplify", () => {
  it("drops sub-threshold jitter but keeps the endpoints", () => {
    const jittery = [p(0, 0), p(0.2, 0.1), p(0.3, 0), p(50, 0), p(50.1, 0.2), p(100, 0)];
    const out = simplify(jittery, 1);
    expect(out.length).toBeLessThan(jittery.length);
    expect(out[0]).toEqual(p(0, 0));
    expect(out[out.length - 1]).toEqual(p(100, 0));
  });

  it("keeps points that clear the threshold", () => {
    const spread = [p(0, 0), p(10, 0), p(20, 0)];
    expect(simplify(spread, 1)).toHaveLength(3);
  });

  it("never disturbs a 2-point path", () => {
    const two = [p(0, 0), p(0.1, 0.1)];
    expect(simplify(two, 5)).toEqual(two);
  });
});

describe("smoothInk", () => {
  it("simplifies then resamples, starting at the raw origin", () => {
    const raw = [p(0, 0, 0.4), p(0.1, 0, 0.4), p(60, 0, 0.7), p(60, 60, 1)];
    const out = smoothInk(raw);
    expect(out[0].x).toBeCloseTo(0);
    expect(out[0].y).toBeCloseTo(0);
    expect(out.length).toBeGreaterThan(2);
  });
});
