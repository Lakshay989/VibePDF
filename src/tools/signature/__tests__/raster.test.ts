// SPEC: P6-SEC-001 (P6.A2) — what `strokesToPng` actually draws.
//
// When this file shipped it had no tests at all, on the grounds that jsdom has
// no canvas. That was true but incomplete: the *pixels* need a canvas, the
// *drawing commands* do not. A recording stub covers the two properties that
// were previously left to someone opening the file in Preview —
//
//   - the background is never filled, so the PNG stays transparent;
//   - the ink is trimmed to its own extent with even padding.
//
// What this still does not prove is that a real 2D context turns those commands
// into the pixels we expect. That gap needs a canvas implementation (a native
// dev dependency); decoding a saved PNG by hand confirmed it once, and those
// numbers are in the commit history.

import { describe, expect, it } from "vitest";

import { RASTER_PAD, TARGET_LONG_EDGE, type Stroke } from "@/tools/signature/draw";
import { strokesToPng } from "@/tools/signature/raster";

interface Recorded {
  calls: string[];
  points: Array<{ x: number; y: number }>;
  widths: number[];
  size: { width: number; height: number };
}

/** A canvas that records instructions instead of rasterising them. */
function recorder(): { createCanvas: () => HTMLCanvasElement; rec: Recorded } {
  const rec: Recorded = { calls: [], points: [], widths: [], size: { width: 0, height: 0 } };

  const ctx = {
    lineCap: "",
    lineJoin: "",
    strokeStyle: "",
    fillStyle: "",
    set lineWidth(v: number) {
      rec.widths.push(v);
    },
    beginPath: () => rec.calls.push("beginPath"),
    moveTo: (x: number, y: number) => {
      rec.calls.push("moveTo");
      rec.points.push({ x, y });
    },
    lineTo: (x: number, y: number) => {
      rec.calls.push("lineTo");
      rec.points.push({ x, y });
    },
    arc: (x: number, y: number) => {
      rec.calls.push("arc");
      rec.points.push({ x, y });
    },
    stroke: () => rec.calls.push("stroke"),
    fill: () => rec.calls.push("fill"),
    // Present so a stray background fill would be recorded rather than crash.
    fillRect: () => rec.calls.push("fillRect"),
    clearRect: () => rec.calls.push("clearRect"),
  };

  const canvas = {
    set width(v: number) {
      rec.size.width = v;
    },
    get width() {
      return rec.size.width;
    },
    set height(v: number) {
      rec.size.height = v;
    },
    get height() {
      return rec.size.height;
    },
    getContext: () => ctx,
    // jsdom's Blob has no `arrayBuffer()` (real browsers do), so the stub
    // supplies one — the production `await blob.arrayBuffer()` still runs.
    toBlob: (cb: (b: Blob | null) => void) =>
      cb({
        arrayBuffer: async () => Uint8Array.from([1, 2, 3]).buffer,
      } as unknown as Blob),
  };

  return { createCanvas: () => canvas as unknown as HTMLCanvasElement, rec };
}

const pt = (x: number, y: number, pressure = 0.5) => ({ x, y, pressure });

/** A wide, shallow squiggle — long edge horizontal. */
const squiggle: Stroke[] = [[pt(20, 60), pt(80, 30), pt(140, 70), pt(200, 40)]];

describe("strokesToPng", () => {
  it("never fills a background — that is what keeps the PNG transparent", async () => {
    const { createCanvas, rec } = recorder();
    await strokesToPng(squiggle, { createCanvas });

    expect(rec.calls).not.toContain("fillRect");
    // A `fill` is legitimate only for the single-dot case, which this is not.
    expect(rec.calls).not.toContain("fill");
  });

  it("sizes the surface from the fit, long edge at the target", async () => {
    const { createCanvas, rec } = recorder();
    await strokesToPng(squiggle, { createCanvas });

    expect(rec.size.width).toBe(TARGET_LONG_EDGE);
    expect(rec.size.height).toBeLessThan(rec.size.width);
  });

  it("draws every point inside the padding — the crop is tight and nothing clips", async () => {
    const { createCanvas, rec } = recorder();
    await strokesToPng(squiggle, { createCanvas });

    expect(rec.points.length).toBeGreaterThan(0);
    for (const p of rec.points) {
      expect(p.x).toBeGreaterThanOrEqual(RASTER_PAD - 0.5);
      expect(p.y).toBeGreaterThanOrEqual(RASTER_PAD - 0.5);
      expect(p.x).toBeLessThanOrEqual(rec.size.width - RASTER_PAD + 0.5);
      expect(p.y).toBeLessThanOrEqual(rec.size.height - RASTER_PAD + 0.5);
    }
    // …and it actually reaches the edges, or "inside the padding" would be
    // satisfied by drawing nothing near them.
    expect(Math.min(...rec.points.map((p) => p.x))).toBeLessThan(RASTER_PAD + 1);
    expect(Math.max(...rec.points.map((p) => p.x))).toBeGreaterThan(rec.size.width - RASTER_PAD - 1);
  });

  it("smooths: it emits far more points than were captured", async () => {
    const { createCanvas, rec } = recorder();
    await strokesToPng(squiggle, { createCanvas });

    // Catmull-Rom resampling at ~3pt spacing over a ~200pt-wide stroke, scaled
    // up to 600px. Four captured points cannot produce a curve on their own.
    expect(rec.points.length).toBeGreaterThan(squiggle[0]!.length * 4);
  });

  it("draws a lone point as a filled dot, not a zero-length line", async () => {
    const { createCanvas, rec } = recorder();
    await strokesToPng([[pt(5, 5)]], { createCanvas });

    // Stroking a path with no length paints nothing at all.
    expect(rec.calls).toContain("arc");
    expect(rec.calls).toContain("fill");
    expect(rec.calls).not.toContain("stroke");
  });

  it("varies width with pressure but never goes hairline", async () => {
    const { createCanvas, rec } = recorder();
    await strokesToPng([[pt(0, 0, 0), pt(50, 10, 0), pt(100, 0, 1)]], { createCanvas });

    // A mouse reports pressure 0; that must not render a near-invisible line.
    const base = 3;
    expect(Math.min(...rec.widths)).toBeGreaterThanOrEqual(base * 1.0);
    expect(Math.max(...rec.widths)).toBeGreaterThan(Math.min(...rec.widths));
  });

  it("refuses to encode when nothing was drawn", async () => {
    const { createCanvas } = recorder();
    await expect(strokesToPng([], { createCanvas })).rejects.toThrow(/nothing drawn/);
    await expect(strokesToPng([[]], { createCanvas })).rejects.toThrow(/nothing drawn/);
  });

  it("surfaces a missing 2D context instead of returning a broken PNG", async () => {
    const createCanvas = () =>
      ({ getContext: () => null, width: 0, height: 0 }) as unknown as HTMLCanvasElement;
    await expect(strokesToPng(squiggle, { createCanvas })).rejects.toThrow(/canvas is unavailable/);
  });

  it("returns the encoded bytes", async () => {
    const { createCanvas } = recorder();
    const png = await strokesToPng(squiggle, { createCanvas });
    expect(png).toBeInstanceOf(Uint8Array);
    expect(Array.from(png)).toEqual([1, 2, 3]);
  });
});
