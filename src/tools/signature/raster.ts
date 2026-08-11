// SPEC: P6-SEC-001 (P6.A2) — rasterise captured strokes to a transparent PNG,
// which is what the signature library stores (P6.A1).
//
// **This file is not unit-tested, and that is a deliberate trade.** jsdom has no
// canvas implementation — `getContext("2d")` returns null — and `canvas` is a
// native dependency this project does not carry for one drawing routine. So
// every *decision* lives in `draw.ts` (bounds, trim, scale, aspect, what counts
// as ink), all of it tested, and what remains here is a straight transcription
// of that fit onto a 2D context with no branching worth asserting. Its
// correctness rests on the acceptance check: open the stored PNG and look at it.
//
// Two properties it must hold, neither visible to a unit test:
//   - the background stays **transparent** (never fill), or every placed
//     signature carries a white box with it;
//   - the ink is trimmed to its own extent, so the stored image is the
//     signature and not the pad it was drawn on.

import { smoothInk } from "@/tools/ink/ink";
import {
  fitToRaster,
  project,
  strokeBounds,
  TARGET_LONG_EDGE,
  type Stroke,
} from "@/tools/signature/draw";

export interface RasterOptions {
  /** Long edge of the output, in pixels. */
  target?: number;
  /** Stroke width at neutral pressure, in raster pixels. */
  lineWidth?: number;
  /** CSS colour for the ink. */
  color?: string;
}

/**
 * Draw `strokes` into a transparent PNG and return its bytes.
 *
 * Each stroke is smoothed with the same `smoothInk` the freehand tool uses
 * (P3-ANN-005) — simplify away jitter, then resample a Catmull-Rom spline — so
 * the result reads as a curve rather than a chain of segments. That is the
 * spec's "with smoothing".
 *
 * Rejects when there is no ink, rather than emitting a degenerate image the
 * library would then have to store.
 */
export async function strokesToPng(
  strokes: readonly Stroke[],
  { target = TARGET_LONG_EDGE, lineWidth = 3, color = "#111" }: RasterOptions = {},
): Promise<Uint8Array> {
  const bounds = strokeBounds(strokes);
  if (!bounds) throw new Error("nothing drawn");

  const fit = fitToRaster(bounds, target);
  const canvas = document.createElement("canvas");
  canvas.width = fit.width;
  canvas.height = fit.height;

  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("2D canvas is unavailable");

  // No fillRect: the canvas starts fully transparent and must stay that way.
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  ctx.strokeStyle = color;

  for (const raw of strokes) {
    if (raw.length === 0) continue;
    const pts = smoothInk(raw);

    // A single point is a dot, not a line — stroking a zero-length path draws
    // nothing, so fill a round cap by hand.
    if (pts.length === 1) {
      const p = project(pts[0]!, fit);
      ctx.beginPath();
      ctx.arc(p.x, p.y, lineWidth / 2, 0, Math.PI * 2);
      ctx.fillStyle = color;
      ctx.fill();
      continue;
    }

    // One path per segment, so width can follow pressure. Pressure is 0.5 at
    // neutral (the freehand tool's convention); a mouse reports 0, which would
    // otherwise render hairline-thin, so it floors at the neutral width.
    for (let i = 1; i < pts.length; i++) {
      const a = project(pts[i - 1]!, fit);
      const b = project(pts[i]!, fit);
      const pressure = Math.max(pts[i]!.pressure, 0.5);
      ctx.lineWidth = lineWidth * (0.5 + pressure);
      ctx.beginPath();
      ctx.moveTo(a.x, a.y);
      ctx.lineTo(b.x, b.y);
      ctx.stroke();
    }
  }

  const blob = await new Promise<Blob | null>((resolve) =>
    canvas.toBlob(resolve, "image/png"),
  );
  if (!blob) throw new Error("could not encode the signature as PNG");
  return new Uint8Array(await blob.arrayBuffer());
}
