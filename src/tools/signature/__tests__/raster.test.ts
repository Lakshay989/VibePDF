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
import { imageToPng, strokesToPng, textToPng } from "@/tools/signature/raster";

interface Recorded {
  calls: string[];
  points: Array<{ x: number; y: number }>;
  widths: number[];
  size: { width: number; height: number };
  /** Every value assigned to ctx.font, in order. */
  fonts: string[];
  /** Arguments of each fillText call. */
  texts: Array<{ text: string; x: number; y: number }>;
}

/** Glyph metrics the stub reports. `null` mimics an engine that omits them. */
interface StubMetrics {
  width: number;
  left: number | null;
  right: number | null;
  ascent: number | null;
  descent: number | null;
}

const DEFAULT_METRICS: StubMetrics = {
  width: 300,
  left: 0,
  right: 300,
  ascent: 70,
  descent: 20,
};

/** A canvas that records instructions instead of rasterising them. */
function recorder(metrics: StubMetrics = DEFAULT_METRICS): {
  createCanvas: () => HTMLCanvasElement;
  rec: Recorded;
} {
  const rec: Recorded = {
    calls: [],
    points: [],
    widths: [],
    size: { width: 0, height: 0 },
    fonts: [],
    texts: [],
  };

  const ctx = {
    lineCap: "",
    lineJoin: "",
    strokeStyle: "",
    fillStyle: "",
    textBaseline: "",
    set font(v: string) {
      rec.fonts.push(v);
    },
    get font() {
      return rec.fonts[rec.fonts.length - 1] ?? "";
    },
    measureText: () => ({
      width: metrics.width,
      actualBoundingBoxLeft: metrics.left,
      actualBoundingBoxRight: metrics.right,
      actualBoundingBoxAscent: metrics.ascent,
      actualBoundingBoxDescent: metrics.descent,
    }),
    fillText: (text: string, x: number, y: number) => {
      rec.calls.push("fillText");
      rec.texts.push({ text, x, y });
    },
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

describe("textToPng", () => {
  it("never fills a background — same transparency rule as the pad", async () => {
    const { createCanvas, rec } = recorder();
    await textToPng("Ada Lovelace", { createCanvas, family: "Snell Roundhand" });

    expect(rec.calls).not.toContain("fillRect");
    expect(rec.calls).toContain("fillText");
  });

  it("draws in the family it was given, with the generic as a safety net", async () => {
    const { createCanvas, rec } = recorder();
    await textToPng("Ada", { createCanvas, family: "Zapfino" });

    expect(rec.fonts.every((f) => f.includes('"Zapfino"'))).toBe(true);
    // If Zapfino vanished between detection and draw, cursive still beats a
    // silent Helvetica.
    expect(rec.fonts.every((f) => f.includes("cursive"))).toBe(true);
  });

  it("sizes the surface from the glyph bounds, long edge at the target", async () => {
    const { createCanvas, rec } = recorder();
    await textToPng("Ada", { createCanvas });

    // 300 wide vs 90 tall → width is the long edge.
    expect(rec.size.width).toBe(TARGET_LONG_EDGE);
    expect(rec.size.height).toBeLessThan(rec.size.width);
  });

  it("crops to the glyphs, not the advance width", async () => {
    // A swash face whose ink starts left of the origin and overshoots the
    // advance: cropping by `width` alone would clip both ends.
    const { createCanvas, rec } = recorder({
      width: 200,
      left: 40,
      right: 260,
      ascent: 70,
      descent: 20,
    });
    await textToPng("Ada", { createCanvas });

    const drawn = rec.texts[0]!;
    // The origin sits `left` in from the padded edge, so the leftward swash
    // lands exactly at the padding rather than off-canvas.
    expect(drawn.x).toBeGreaterThan(RASTER_PAD);
  });

  it("falls back to em-box estimates when the engine reports no glyph metrics", async () => {
    const { createCanvas, rec } = recorder({
      width: 300,
      left: null,
      right: null,
      ascent: null,
      descent: null,
    });
    await textToPng("Ada", { createCanvas });

    // Loose, but nothing is clipped and nothing divides by zero.
    expect(rec.size.width).toBe(TARGET_LONG_EDGE);
    expect(rec.size.height).toBeGreaterThan(2 * RASTER_PAD);
  });

  it("refuses empty or whitespace-only text", async () => {
    const { createCanvas } = recorder();
    await expect(textToPng("", { createCanvas })).rejects.toThrow(/nothing typed/);
    await expect(textToPng("   \n\t ", { createCanvas })).rejects.toThrow(/nothing typed/);
  });

  it("refuses when the font produced no glyphs at all", async () => {
    // Zero-width text is what a face with no coverage for the script gives.
    const { createCanvas } = recorder({ width: 0, left: 0, right: 0, ascent: 0, descent: 0 });
    await expect(textToPng("आदित्य", { createCanvas })).rejects.toThrow(/no glyphs/);
  });

  it("trims surrounding whitespace before drawing", async () => {
    const { createCanvas, rec } = recorder();
    await textToPng("  Ada  ", { createCanvas });
    expect(rec.texts[0]!.text).toBe("Ada");
  });

  it("surfaces a missing 2D context", async () => {
    const createCanvas = () =>
      ({ getContext: () => null, width: 0, height: 0 }) as unknown as HTMLCanvasElement;
    await expect(textToPng("Ada", { createCanvas })).rejects.toThrow(/canvas is unavailable/);
  });
});

// SPEC: P6-SEC-003 (P6.A4) — importing an image.
//
// Two seams here rather than one: `createCanvas` as above, and `decode`,
// because `createImageBitmap` is a browser API jsdom does not implement. What
// the threshold *decides* is covered properly in `threshold.test.ts` against
// real pixels; what is left for this stub is the plumbing around it — the sizes
// chosen, the crop taken, and the same never-fill rule as the other two.

interface ImageRecorded {
  /** One entry per canvas created, in order: working surface, then output. */
  canvases: Array<{ width: number; height: number }>;
  calls: string[];
  /** Numeric arguments of each `drawImage`, source omitted. */
  draws: number[][];
}

/** A frame the stub hands back from `getImageData`, built pixel by pixel. */
function frameOf(
  width: number,
  height: number,
  at: (x: number, y: number) => [number, number, number, number],
) {
  const data = new Uint8ClampedArray(width * height * 4);
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const i = (y * width + x) * 4;
      const [r, g, b, a] = at(x, y);
      data[i] = r;
      data[i + 1] = g;
      data[i + 2] = b;
      data[i + 3] = a;
    }
  }
  return { width, height, data };
}

function imageRecorder(frame: ReturnType<typeof frameOf>): {
  createCanvas: () => HTMLCanvasElement;
  rec: ImageRecorded;
} {
  const rec: ImageRecorded = { canvases: [], calls: [], draws: [] };

  const createCanvas = () => {
    const size = { width: 0, height: 0 };
    rec.canvases.push(size);
    const ctx = {
      drawImage: (_source: unknown, ...args: number[]) => {
        rec.calls.push("drawImage");
        rec.draws.push(args);
      },
      getImageData: () => frame,
      putImageData: () => rec.calls.push("putImageData"),
      // Present so a stray background fill is recorded rather than crashing.
      fillRect: () => rec.calls.push("fillRect"),
      clearRect: () => rec.calls.push("clearRect"),
    };
    return {
      set width(v: number) {
        size.width = v;
      },
      get width() {
        return size.width;
      },
      set height(v: number) {
        size.height = v;
      },
      get height() {
        return size.height;
      },
      getContext: () => ctx,
      toBlob: (cb: (b: Blob | null) => void) =>
        cb({ arrayBuffer: async () => Uint8Array.from([9, 9]).buffer } as unknown as Blob),
    } as unknown as HTMLCanvasElement;
  };

  return { createCanvas, rec };
}

/** A decoder that reports a natural size without touching any bytes. */
const decoder = (width: number, height: number) => async () => ({
  width,
  height,
  source: {} as CanvasImageSource,
});

const bytes = Uint8Array.from([0x89, 0x50, 0x4e, 0x47]);

/** 10×10, with ink filling x∈[2,5], y∈[3,4] — a 4×2 box off-centre. */
const inkAt = (x: number, y: number): [number, number, number, number] =>
  x >= 2 && x <= 5 && y >= 3 && y <= 4 ? [10, 10, 10, 255] : [0, 0, 0, 0];

describe("imageToPng", () => {
  it("never fills a background — same transparency rule as the pad", async () => {
    const { createCanvas, rec } = imageRecorder(frameOf(10, 10, inkAt));
    await imageToPng(bytes, { createCanvas, decode: decoder(10, 10) });

    expect(rec.calls).not.toContain("fillRect");
    expect(rec.calls).toContain("drawImage");
  });

  it("sizes the output from the opaque pixels, not the source frame", async () => {
    const { createCanvas, rec } = imageRecorder(frameOf(10, 10, inkAt));
    await imageToPng(bytes, { createCanvas, decode: decoder(10, 10) });

    // The working surface is the whole image…
    expect(rec.canvases[0]).toEqual({ width: 10, height: 10 });
    // …the output is the 4×2 of ink, plus padding on every side.
    expect(rec.canvases[1]).toEqual({
      width: 4 + 2 * RASTER_PAD,
      height: 2 + 2 * RASTER_PAD,
    });
  });

  it("crops to the ink and lands it inside the padding", async () => {
    const { createCanvas, rec } = imageRecorder(frameOf(10, 10, inkAt));
    await imageToPng(bytes, { createCanvas, decode: decoder(10, 10) });

    // sx, sy, sw, sh, dx, dy, dw, dh — the source rect is the ink box, and the
    // destination starts exactly one pad in.
    expect(rec.draws[1]).toEqual([2, 3, 4, 2, RASTER_PAD, RASTER_PAD, 4, 2]);
  });

  it("downscales a large source before the threshold ever runs", async () => {
    const { createCanvas, rec } = imageRecorder(frameOf(600, 450, () => [10, 10, 10, 255]));
    await imageToPng(bytes, { createCanvas, decode: decoder(4000, 3000) });

    // 12 megapixels would be ~48MB of RGBA to walk on every slider tick.
    expect(rec.canvases[0]).toEqual({ width: TARGET_LONG_EDGE, height: 450 });
  });

  it("never upscales a small crop", async () => {
    const { createCanvas, rec } = imageRecorder(frameOf(10, 10, inkAt));
    await imageToPng(bytes, { createCanvas, decode: decoder(10, 10) });

    // Blowing 4px of ink up to 600 would add blur and nothing else.
    expect(rec.canvases[1]!.width).toBeLessThan(TARGET_LONG_EDGE);
  });

  it("reports what the threshold removed, so the UI can say so", async () => {
    // Top row white, bottom row ink; a mid threshold takes the white.
    const frame = frameOf(2, 2, (_x, y) =>
      y === 0 ? [255, 255, 255, 255] : [10, 10, 10, 255],
    );
    const { createCanvas } = imageRecorder(frame);
    const out = await imageToPng(bytes, { createCanvas, decode: decoder(2, 2), strength: 50 });

    expect(out).toMatchObject({ erased: 2, total: 4, transparent: 2 });
    expect(Array.from(out.png)).toEqual([9, 9]);
  });

  it("warns by way of `transparent` when an image has no alpha at all", async () => {
    const { createCanvas } = imageRecorder(frameOf(2, 2, () => [10, 10, 10, 255]));
    const out = await imageToPng(bytes, { createCanvas, decode: decoder(2, 2) });

    // The JPEG case: placed as-is it would be a solid rectangle.
    expect(out.transparent).toBe(0);
    expect(out.erased).toBe(0);
  });

  it("refuses when the threshold erased the whole image", async () => {
    const { createCanvas } = imageRecorder(frameOf(2, 2, () => [10, 10, 10, 255]));
    await expect(
      imageToPng(bytes, { createCanvas, decode: decoder(2, 2), strength: 100 }),
    ).rejects.toThrow(/erased the whole image/);
  });

  it("refuses a file that did not decode to anything", async () => {
    const { createCanvas } = imageRecorder(frameOf(2, 2, () => [10, 10, 10, 255]));
    await expect(
      imageToPng(bytes, { createCanvas, decode: decoder(0, 0) }),
    ).rejects.toThrow(/could not be read as an image/);
  });

  it("surfaces a decoder failure rather than swallowing it", async () => {
    const { createCanvas } = imageRecorder(frameOf(2, 2, () => [10, 10, 10, 255]));
    const decode = () => Promise.reject(new Error("unsupported image format"));
    await expect(imageToPng(bytes, { createCanvas, decode })).rejects.toThrow(/unsupported/);
  });

  it("surfaces a missing 2D context", async () => {
    const createCanvas = () =>
      ({ getContext: () => null, width: 0, height: 0 }) as unknown as HTMLCanvasElement;
    await expect(
      imageToPng(bytes, { createCanvas, decode: decoder(4, 4) }),
    ).rejects.toThrow(/canvas is unavailable/);
  });
});
