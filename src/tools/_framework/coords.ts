// SPEC: infrastructure (P3.A1) — screen ↔ PDF coordinate mapping for a page.
//
// Annotations are stored in PDF user space (origin bottom-left, y up, points),
// but pointer events arrive in the rendered element's CSS pixels (origin
// top-left, y down) and the page may be displayed rotated. This module is the
// one place that mapping lives, so the classic "annotation lands in the wrong
// spot" bugs (y-flip, scale, rotation) are isolated and round-trip-tested.
//
// Rotation follows PDF `/Rotate` (clockwise). The mapping is verified for
// self-consistency here (screen → PDF → screen ≈ identity) and against real
// rendering in P3.A2/B1.

import type { PagePoint, PdfRect } from "./types";

/** Geometry of a rendered page. `width`/`height` are the *unrotated* PDF size. */
export interface PageGeometry {
  /** 0-based page index. */
  page: number;
  width: number;
  height: number;
  /** CSS pixels per PDF point. */
  scale: number;
  /** Display rotation in degrees; snapped to 0 | 90 | 180 | 270. */
  rotation: number;
}

/** A point in the rendered page element's CSS pixels (origin top-left, y down). */
export interface ScreenPoint {
  x: number;
  y: number;
}

/** Snap any degree value to one of 0 / 90 / 180 / 270. */
function quarter(deg: number): 0 | 90 | 180 | 270 {
  return ((((Math.round(deg / 90) % 4) + 4) % 4) * 90) as 0 | 90 | 180 | 270;
}

/** Map a rendered-element CSS point to PDF page coordinates. */
export function screenToPdf(s: ScreenPoint, geo: PageGeometry): PagePoint {
  const { width: w, height: h, scale, page } = geo;
  const sx = s.x / scale;
  const sy = s.y / scale;
  switch (quarter(geo.rotation)) {
    case 90:
      return { page, x: sy, y: sx };
    case 180:
      return { page, x: w - sx, y: sy };
    case 270:
      return { page, x: w - sy, y: h - sx };
    default:
      return { page, x: sx, y: h - sy };
  }
}

/** Map a PDF page point to the rendered element's CSS pixels. */
export function pdfToScreen(p: PagePoint, geo: PageGeometry): ScreenPoint {
  const { width: w, height: h, scale } = geo;
  switch (quarter(geo.rotation)) {
    case 90:
      return { x: p.y * scale, y: p.x * scale };
    case 180:
      return { x: (w - p.x) * scale, y: p.y * scale };
    case 270:
      return { x: (h - p.y) * scale, y: (w - p.x) * scale };
    default:
      return { x: p.x * scale, y: (h - p.y) * scale };
  }
}

/** A normalized PDF rect (`x0 ≤ x1`, `y0 ≤ y1`) spanning two page points. */
export function pdfRectFromPoints(a: PagePoint, b: PagePoint): PdfRect {
  return {
    x0: Math.min(a.x, b.x),
    y0: Math.min(a.y, b.y),
    x1: Math.max(a.x, b.x),
    y1: Math.max(a.y, b.y),
  };
}

/** Normalize a rect so `x0 ≤ x1` and `y0 ≤ y1`. */
export function normalizeRect(r: PdfRect): PdfRect {
  return {
    x0: Math.min(r.x0, r.x1),
    y0: Math.min(r.y0, r.y1),
    x1: Math.max(r.x0, r.x1),
    y1: Math.max(r.y0, r.y1),
  };
}

// --- Screen-space drag rects (framework-level; used by every rect-drawing
// layer — free-text, text box, link, image add). FABLE_REVIEW §3.15. ---

/** A rectangle in CSS px within a page layer. */
export interface ScreenRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** The minimum box (CSS px) a drag must reach; a smaller drag is a "click". */
export const MIN_DRAG_PX = 8;

/** Default box (CSS px) used when the gesture was a click, not a real drag. */
export const DEFAULT_BOX_PX = { width: 160, height: 28 } as const;

/** Normalize two pointer points (CSS px, layer-relative) into a positive rect. */
export function normalizeScreenRect(
  a: { x: number; y: number },
  b: { x: number; y: number },
): ScreenRect {
  const left = Math.min(a.x, b.x);
  const top = Math.min(a.y, b.y);
  return { left, top, width: Math.abs(b.x - a.x), height: Math.abs(b.y - a.y) };
}

/**
 * If `rect` is smaller than `MIN_DRAG_PX` in either axis (a click, not a drag),
 * grow it to `DEFAULT_BOX_PX` anchored at its top-left so the user still gets a
 * usable box.
 */
export function withDefaultSize(rect: ScreenRect): ScreenRect {
  if (rect.width >= MIN_DRAG_PX && rect.height >= MIN_DRAG_PX) return rect;
  return { left: rect.left, top: rect.top, width: DEFAULT_BOX_PX.width, height: DEFAULT_BOX_PX.height };
}
