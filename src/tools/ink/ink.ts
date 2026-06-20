// SPEC: P3-ANN-005 — freehand ink smoothing. The pen captures a noisy, unevenly
// spaced stream of pointer samples; before we persist it as a PDF `/Ink`
// annotation we (1) drop sub-threshold jitter, then (2) resample a Catmull-Rom
// spline through the survivors at an even spacing. The dense, even result is what
// makes the variable-width ribbon `/AP` (drawn on the Rust side) read as a smooth
// curve rather than a chain of straight segments. Pure + framework-free so it can
// be unit-tested in isolation (see `__tests__/smoothing.test.ts`).

/** A captured pen sample in PDF points; `pressure` in `[0,1]` (0.5 == neutral). */
export interface InkPoint {
  x: number;
  y: number;
  pressure: number;
}

/** Drop jitter (<1pt) and resample the spline at ~3pt spacing. */
const DEFAULT_MIN_DIST = 1;
const DEFAULT_SPACING = 3;

/**
 * Remove points within `minDist` (PDF points) of the previously kept point —
 * the high-frequency jitter a trembling hand or a high-rate digitizer emits.
 * The first and last samples are always kept so the stroke's extent is
 * preserved. Pressure rides along from whichever sample survives.
 */
export function simplify(points: readonly InkPoint[], minDist = DEFAULT_MIN_DIST): InkPoint[] {
  if (points.length <= 2) return [...points];
  const out: InkPoint[] = [points[0]];
  for (let i = 1; i < points.length - 1; i++) {
    const p = points[i];
    const last = out[out.length - 1];
    if (Math.hypot(p.x - last.x, p.y - last.y) >= minDist) out.push(p);
  }
  out.push(points[points.length - 1]);
  return out;
}

/**
 * Resample a uniform Catmull-Rom spline through `points` at roughly `spacing`
 * PDF points. The curve passes through every input point (interpolating, not
 * approximating); endpoints are clamped by duplicating the first/last control
 * point. Pressure is linearly interpolated along each segment (a Catmull-Rom on
 * pressure could overshoot out of `[0,1]`). Degenerate inputs (0–1 points) pass
 * straight through.
 */
export function catmullRomResample(points: readonly InkPoint[], spacing = DEFAULT_SPACING): InkPoint[] {
  if (points.length <= 1) return [...points];

  const step = Math.max(spacing, 0.1);
  const out: InkPoint[] = [points[0]];

  for (let i = 0; i < points.length - 1; i++) {
    const p0 = points[i - 1] ?? points[i];
    const p1 = points[i];
    const p2 = points[i + 1];
    const p3 = points[i + 2] ?? points[i + 1];

    const segLen = Math.hypot(p2.x - p1.x, p2.y - p1.y);
    const steps = Math.max(1, Math.ceil(segLen / step));

    for (let s = 1; s <= steps; s++) {
      const t = s / steps;
      out.push({
        x: catmullRom(p0.x, p1.x, p2.x, p3.x, t),
        y: catmullRom(p0.y, p1.y, p2.y, p3.y, t),
        // Linear in pressure between the two segment endpoints.
        pressure: p1.pressure + (p2.pressure - p1.pressure) * t,
      });
    }
  }

  return out;
}

/** Simplify then resample — the full pen-to-persisted pipeline. */
export function smoothInk(
  points: readonly InkPoint[],
  minDist = DEFAULT_MIN_DIST,
  spacing = DEFAULT_SPACING,
): InkPoint[] {
  return catmullRomResample(simplify(points, minDist), spacing);
}

/** Uniform Catmull-Rom basis (tension 0.5) evaluated at `t ∈ [0,1]`. */
function catmullRom(a: number, b: number, c: number, d: number, t: number): number {
  const t2 = t * t;
  const t3 = t2 * t;
  return 0.5 * (2 * b + (-a + c) * t + (2 * a - 5 * b + 4 * c - d) * t2 + (-a + 3 * b - 3 * c + d) * t3);
}
