// SPEC: P3-ANN-007 — calibration + the distance / perimeter / area maths.

import { describe, expect, it } from "vitest";

import {
  calibrationScale,
  DEFAULT_CALIBRATION,
  formatMeasurement,
  measureValue,
  minPoints,
  pathLength,
  polygonArea,
  type Point,
  straightDistance,
} from "@/tools/measure/measure";

const p = (x: number, y: number): Point => ({ x, y });

describe("calibrationScale", () => {
  it("maps a drawn reference to real units (50pt = 1m → 0.02 m/pt)", () => {
    expect(calibrationScale(50, 1)).toBeCloseTo(0.02);
  });

  it("falls back to 1:1 on a zero/negative reference", () => {
    expect(calibrationScale(0, 5)).toBe(DEFAULT_CALIBRATION.unitsPerPoint);
    expect(calibrationScale(-3, 5)).toBe(DEFAULT_CALIBRATION.unitsPerPoint);
  });
});

describe("geometry", () => {
  it("straightDistance is the first→last segment", () => {
    expect(straightDistance([p(0, 0), p(3, 4)])).toBeCloseTo(5);
    expect(straightDistance([p(0, 0)])).toBe(0);
  });

  it("pathLength sums every segment", () => {
    expect(pathLength([p(0, 0), p(3, 4), p(3, 4 + 10)])).toBeCloseTo(15);
  });

  it("polygonArea uses the shoelace formula (abs)", () => {
    // A 10×10 square = 100, regardless of winding direction.
    const ccw = [p(0, 0), p(10, 0), p(10, 10), p(0, 10)];
    expect(polygonArea(ccw)).toBeCloseTo(100);
    expect(polygonArea([...ccw].reverse())).toBeCloseTo(100);
    expect(polygonArea([p(0, 0), p(1, 1)])).toBe(0); // < 3 points
  });
});

describe("measureValue + formatMeasurement", () => {
  const cal = { unitsPerPoint: 0.02, unit: "m" }; // 50pt = 1m

  it("scales distance and perimeter linearly", () => {
    expect(measureValue("distance", [p(0, 0), p(50, 0)], cal)).toBeCloseTo(1);
    expect(measureValue("perimeter", [p(0, 0), p(50, 0), p(50, 50)], cal)).toBeCloseTo(2);
  });

  it("scales area by the square of the unit scale", () => {
    // 50×50 pt = 2500 pt² → 1 m².
    const area = measureValue("area", [p(0, 0), p(50, 0), p(50, 50), p(0, 50)], cal);
    expect(area).toBeCloseTo(1);
  });

  it("formats with the unit (area uses unit²) rounded to 2dp", () => {
    expect(formatMeasurement("distance", 3.20001, "m")).toBe("3.2 m");
    expect(formatMeasurement("area", 1, "m")).toBe("1 m²");
  });

  it("minPoints: area needs 3, others 2", () => {
    expect(minPoints("distance")).toBe(2);
    expect(minPoints("perimeter")).toBe(2);
    expect(minPoints("area")).toBe(3);
  });
});
