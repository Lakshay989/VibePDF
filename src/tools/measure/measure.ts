// SPEC: P3-ANN-007 (P3.C4a) — measurement maths + calibration. Pure/DOM-free so
// the distance / perimeter / area + scale logic is unit-testable in isolation
// (see `__tests__/calibration.test.ts`). All geometry is in PDF points; the
// calibration converts points → real-world units.

export type MeasureKind = "distance" | "perimeter" | "area";

export interface Point {
  x: number;
  y: number;
}

/** A user-specified scale: real-world `unit`s per PDF point. */
export interface Calibration {
  unitsPerPoint: number;
  unit: string;
}

/** Uncalibrated: 1 point reads as 1 "pt". */
export const DEFAULT_CALIBRATION: Calibration = { unitsPerPoint: 1, unit: "pt" };

function segmentLength(a: Point, b: Point): number {
  return Math.hypot(b.x - a.x, b.y - a.y);
}

/** Total length of the path through `points`, in points. */
export function pathLength(points: readonly Point[]): number {
  let total = 0;
  for (let i = 1; i < points.length; i += 1) total += segmentLength(points[i - 1], points[i]);
  return total;
}

/** Straight-line distance between the first and last point, in points. */
export function straightDistance(points: readonly Point[]): number {
  if (points.length < 2) return 0;
  return segmentLength(points[0], points[points.length - 1]);
}

/** Polygon area in points², via the shoelace formula. Absolute (sign-agnostic);
 *  undefined for a self-intersecting ring. */
export function polygonArea(points: readonly Point[]): number {
  const n = points.length;
  if (n < 3) return 0;
  let sum = 0;
  for (let i = 0; i < n; i += 1) {
    const a = points[i];
    const b = points[(i + 1) % n];
    sum += a.x * b.y - b.x * a.y;
  }
  return Math.abs(sum) / 2;
}

/** The scale mapping a drawn reference of `referencePoints` to `realLength` real
 *  units. Guards a zero/negative reference (falls back to 1:1). */
export function calibrationScale(referencePoints: number, realLength: number): number {
  if (referencePoints <= 0) return DEFAULT_CALIBRATION.unitsPerPoint;
  return realLength / referencePoints;
}

/** The measured value (real units, or unit² for area) for `kind`. */
export function measureValue(kind: MeasureKind, points: readonly Point[], cal: Calibration): number {
  switch (kind) {
    case "distance":
      return straightDistance(points) * cal.unitsPerPoint;
    case "perimeter":
      return pathLength(points) * cal.unitsPerPoint;
    case "area":
      return polygonArea(points) * cal.unitsPerPoint * cal.unitsPerPoint;
  }
}

/** Format a measured value with its unit (area uses `unit²`), rounded to 2dp. */
export function formatMeasurement(kind: MeasureKind, value: number, unit: string): string {
  const rounded = Math.round(value * 100) / 100;
  return `${rounded} ${kind === "area" ? `${unit}²` : unit}`;
}

/** Minimum vertices to finish a measurement of `kind`. */
export function minPoints(kind: MeasureKind): number {
  return kind === "area" ? 3 : 2;
}
