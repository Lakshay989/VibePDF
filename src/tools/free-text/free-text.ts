// SPEC: P3-ANN-003 (P3.B3a) — pure helpers for the free-text tool.
//
// The free-text box is a drag gesture (like the shape tools), but the committed
// box is canvas-rendered from a generated `/AP` (like markup) — only the
// transient editor lives in the overlay. These DOM-free helpers (font catalog,
// CSS mapping, screen-rect math) keep the gesture logic unit-testable.

import type { FontFamily } from "@/tools/_framework";

/** Families offered in the toolbar — the base-14 set we can draw without embedding. */
export const FONT_FAMILIES: readonly FontFamily[] = ["Helvetica", "Times", "Courier"];

/** A CSS font stack approximating each base-14 family for the editor preview. */
export function cssFontFamily(family: FontFamily): string {
  switch (family) {
    case "Times":
      return "'Times New Roman', Times, serif";
    case "Courier":
      return "'Courier New', Courier, monospace";
    case "Helvetica":
    default:
      return "Helvetica, Arial, sans-serif";
  }
}

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
 * usable text box.
 */
export function withDefaultSize(rect: ScreenRect): ScreenRect {
  if (rect.width >= MIN_DRAG_PX && rect.height >= MIN_DRAG_PX) return rect;
  return { left: rect.left, top: rect.top, width: DEFAULT_BOX_PX.width, height: DEFAULT_BOX_PX.height };
}
