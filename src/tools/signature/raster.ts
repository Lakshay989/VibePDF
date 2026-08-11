// SPEC: P6-SEC-001 (P6.A2) — rasterise captured strokes to a transparent PNG,
// which is what the signature library stores (P6.A1).
//
// jsdom has no canvas implementation — `getContext("2d")` returns null — and
// `canvas` is a native dependency this project does not carry for one drawing
// routine. That shaped the design: every *decision* lives in `draw.ts` (bounds,
// trim, scale, aspect, what counts as ink), so what remains here is a
// transcription of that fit onto a 2D context.
//
// It shipped with no tests at all on those grounds, which was an overstatement.
// The *pixels* need a canvas; the *drawing commands* do not. `RasterOptions
// .createCanvas` is a seam a recording stub can occupy, and
// `__tests__/raster.test.ts` uses it to pin the two properties that were
// previously left to manual inspection:
//   - the background is never filled, so the PNG stays **transparent** — else
//     every placed signature carries a white box with it;
//   - the ink is trimmed to its own extent with even padding, so the stored
//     image is the signature and not the pad it was drawn on.
//
// What is still unproven here is that a real 2D context turns those commands
// into the pixels expected. Closing that needs a canvas implementation; a
// hand-decode of a saved PNG confirmed it once (RGBA, corner alpha 0, 10px
// margins on all four sides).

import { smoothInk } from "@/tools/ink/ink";
import {
  fitToRaster,
  project,
  RASTER_PAD,
  strokeBounds,
  TARGET_LONG_EDGE,
  type Stroke,
} from "@/tools/signature/draw";
import {
  applyThreshold,
  countTransparent,
  opaqueBounds,
  strengthToThreshold,
} from "@/tools/signature/threshold";

export interface RasterOptions {
  /** Long edge of the output, in pixels. */
  target?: number;
  /** Stroke width at neutral pressure, in raster pixels. */
  lineWidth?: number;
  /** CSS colour for the ink. */
  color?: string;
  /**
   * How to obtain the drawing surface. Defaults to a real `<canvas>`.
   *
   * The seam exists so tests can pass a recording stub and assert what this
   * function *draws* — that it never fills a background, that the surface is
   * the size the fit asked for, that every point lands inside the padding.
   * That is not the same as asserting pixels, but it covers the decisions,
   * and it needs no canvas implementation to do it.
   */
  createCanvas?: () => HTMLCanvasElement;
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
  {
    target = TARGET_LONG_EDGE,
    lineWidth = 3,
    color = "#111",
    createCanvas = () => document.createElement("canvas"),
  }: RasterOptions = {},
): Promise<Uint8Array> {
  const bounds = strokeBounds(strokes);
  if (!bounds) throw new Error("nothing drawn");

  const fit = fitToRaster(bounds, target);
  const canvas = createCanvas();
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


/** Font size the text is measured at before being scaled to the target. */
const PROBE_PX = 100;

/** Fractions of the em box used when a browser gives no glyph metrics. */
const EST_ASCENT = 0.8;
const EST_DESCENT = 0.2;

export interface TextRasterOptions extends RasterOptions {
  /** CSS family name. Quoted at use site; pass the bare name. */
  family?: string;
}

/**
 * SPEC: P6-SEC-002 (P6.A3) — render `text` in `family` to a transparent PNG.
 *
 * Shares everything structural with `strokesToPng`: the same `createCanvas`
 * seam, the same `fitToRaster` trim-and-scale, the same never-fill rule. Only
 * the source of ink differs — glyphs instead of strokes.
 *
 * The crop comes from `measureText`'s glyph bounds rather than the advance
 * width, so a script face with long swashes or a deep descender is not clipped.
 * Those metrics are widely but not universally implemented; when they are
 * missing or zero the em-box estimates above stand in, which is loose but never
 * cuts anything off.
 */
export async function textToPng(
  text: string,
  {
    target = TARGET_LONG_EDGE,
    color = "#111",
    family = "cursive",
    createCanvas = () => document.createElement("canvas"),
  }: TextRasterOptions = {},
): Promise<Uint8Array> {
  const line = text.trim();
  if (line.length === 0) throw new Error("nothing typed");

  const canvas = createCanvas();
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("2D canvas is unavailable");

  const font = (px: number) => `${px}px "${family}", cursive`;
  ctx.font = font(PROBE_PX);
  const m = ctx.measureText(line);

  // Glyph bounds around the text origin (baseline, left edge). `left` grows
  // leftward, so it is negated to give a box in the same space as the strokes.
  const left = m.actualBoundingBoxLeft || 0;
  const right = m.actualBoundingBoxRight || m.width || 0;
  const ascent = m.actualBoundingBoxAscent || PROBE_PX * EST_ASCENT;
  const descent = m.actualBoundingBoxDescent || PROBE_PX * EST_DESCENT;

  const width = left + right;
  if (width <= 0) throw new Error("the font produced no glyphs for this text");

  const fit = fitToRaster(
    { x: -left, y: -ascent, width, height: ascent + descent },
    target,
  );
  canvas.width = fit.width;
  canvas.height = fit.height;

  // Setting the canvas size resets the context, so state goes on afterwards.
  // No fillRect: the surface starts transparent and must stay that way.
  ctx.font = font(PROBE_PX * fit.scale);
  ctx.fillStyle = color;
  ctx.textBaseline = "alphabetic";
  ctx.fillText(line, fit.offsetX, fit.offsetY);

  const blob = await new Promise<Blob | null>((resolve) =>
    canvas.toBlob(resolve, "image/png"),
  );
  if (!blob) throw new Error("could not encode the signature as PNG");
  return new Uint8Array(await blob.arrayBuffer());
}

/** Long edge the threshold runs over, before the crop. */
const WORK_LONG_EDGE = TARGET_LONG_EDGE;

/** A decoded image and its natural size. `source` is whatever `drawImage`
 *  accepts — an `ImageBitmap` in the app, a stub under test. */
export interface DecodedImage {
  width: number;
  height: number;
  source: CanvasImageSource;
}

export interface ImageRasterOptions extends RasterOptions {
  /** Background removal, 0 (leave the image alone) to 100 (erase everything). */
  strength?: number;
  /**
   * How to turn encoded bytes into something drawable. The seam exists for the
   * same reason `createCanvas` does — `createImageBitmap` is a browser API that
   * jsdom does not implement.
   */
  decode?: (bytes: Uint8Array) => Promise<DecodedImage>;
}

/** The encoded signature plus what the threshold did, which the UI reports. */
export interface ImageRaster {
  png: Uint8Array;
  /** Pixels the threshold removed. */
  erased: number;
  /** Pixels examined, so `erased` can be stated as a proportion. */
  total: number;
  /** Transparent pixels afterwards. Zero means a solid rectangle. */
  transparent: number;
}

/**
 * Decode with the engine's own image support — which is the whole argument for
 * doing this here rather than in Rust. PNG, JPEG and BMP all arrive decoded
 * without a single new dependency, and the format is sniffed from the bytes
 * rather than trusted from the file extension.
 */
async function decodeImage(bytes: Uint8Array): Promise<DecodedImage> {
  const bitmap = await createImageBitmap(new Blob([bytes as BlobPart]));
  return { width: bitmap.width, height: bitmap.height, source: bitmap };
}

/**
 * SPEC: P6-SEC-003 (P6.A4) — import `bytes` as a signature, optionally lifting
 * its background out with a threshold.
 *
 * Same shape as its two siblings: the `createCanvas` seam, `fitToRaster`, and
 * the never-fill rule that keeps the stored PNG transparent. The ink here comes
 * from a file rather than from a pointer or a font.
 *
 * Order matters. The image is downscaled to the working size *before* the
 * threshold runs — a 12-megapixel phone photo is ~48 MB of RGBA, and the loop
 * would run twelve million times on every tick of the slider. At the working
 * size it is ~0.2 MP no matter what was imported.
 */
export async function imageToPng(
  bytes: Uint8Array,
  {
    target = TARGET_LONG_EDGE,
    strength = 0,
    createCanvas = () => document.createElement("canvas"),
    decode = decodeImage,
  }: ImageRasterOptions = {},
): Promise<ImageRaster> {
  const img = await decode(bytes);
  if (img.width <= 0 || img.height <= 0) {
    throw new Error("that file could not be read as an image");
  }

  const shrink = Math.min(1, WORK_LONG_EDGE / Math.max(img.width, img.height));
  const workW = Math.max(1, Math.round(img.width * shrink));
  const workH = Math.max(1, Math.round(img.height * shrink));

  const work = createCanvas();
  work.width = workW;
  work.height = workH;
  const wctx = work.getContext("2d");
  if (!wctx) throw new Error("2D canvas is unavailable");
  wctx.drawImage(img.source, 0, 0, workW, workH);

  const frame = wctx.getImageData(0, 0, workW, workH);
  const erased = applyThreshold(frame.data, strengthToThreshold(strength));
  const transparent = countTransparent(frame.data);
  const box = opaqueBounds(frame.data, workW, workH);
  // Storing a blank PNG would be worse than refusing: the entry would look like
  // a signature in the library and place as nothing.
  if (!box) throw new Error("the background removal erased the whole image");
  wctx.putImageData(frame, 0, 0);

  // Never upscale. Blowing a 120px crop up to 600 adds no detail, only blur —
  // so the target is capped at what the ink actually measures.
  const longest = Math.max(box.width, box.height);
  const fit = fitToRaster(box, Math.min(target, longest + 2 * RASTER_PAD));

  const out = createCanvas();
  out.width = fit.width;
  out.height = fit.height;
  const octx = out.getContext("2d");
  if (!octx) throw new Error("2D canvas is unavailable");
  // No fillRect, for the third time and the same reason.
  octx.drawImage(
    work,
    box.x,
    box.y,
    box.width,
    box.height,
    RASTER_PAD,
    RASTER_PAD,
    box.width * fit.scale,
    box.height * fit.scale,
  );

  const blob = await new Promise<Blob | null>((resolve) => out.toBlob(resolve, "image/png"));
  if (!blob) throw new Error("could not encode the signature as PNG");
  return {
    png: new Uint8Array(await blob.arrayBuffer()),
    erased,
    total: workW * workH,
    transparent,
  };
}
