// SPEC: P3-ANN-001 (P3.B1a) — turn selected-text rectangles into PDF
// `/QuadPoints`. A browser text selection exposes one client rect per line
// (`Range.getClientRects()`); each becomes a quad. This module is pure (rects +
// page geometry in, quads out) so the corner-order/rotation math — the part most
// prone to "highlight renders as a bowtie" bugs — is unit-tested without a DOM.

import { type PageGeometry, screenToPdf } from "@/tools/_framework";
import type { Quad } from "@/tools/_framework";

/** A rectangle in CSS pixels relative to the page element's top-left. */
export interface LocalRect {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

/**
 * One line rect → one quad in PDF points, in `/QuadPoints` order
 * (UL, UR, LL, LR). Each corner is mapped through `screenToPdf`, so rotation
 * and the y-flip are handled by the shared coords module.
 */
export function rectToQuad(r: LocalRect, geo: PageGeometry): Quad {
  const ul = screenToPdf({ x: r.left, y: r.top }, geo);
  const ur = screenToPdf({ x: r.right, y: r.top }, geo);
  const ll = screenToPdf({ x: r.left, y: r.bottom }, geo);
  const lr = screenToPdf({ x: r.right, y: r.bottom }, geo);
  return [ul.x, ul.y, ur.x, ur.y, ll.x, ll.y, lr.x, lr.y];
}

/** Map a set of line rects (already page-local) to quads, dropping empties. */
export function rectsToQuads(rects: LocalRect[], geo: PageGeometry): Quad[] {
  return rects
    .filter((r) => r.right - r.left > 0.5 && r.bottom - r.top > 0.5)
    .map((r) => rectToQuad(r, geo));
}

/** The bounding `/Rect` [x0,y0,x1,y1] (PDF pts) enclosing all quads. */
export function quadsBounds(quads: Quad[]): [number, number, number, number] {
  let x0 = Infinity;
  let y0 = Infinity;
  let x1 = -Infinity;
  let y1 = -Infinity;
  for (const q of quads) {
    for (let i = 0; i < 8; i += 2) {
      x0 = Math.min(x0, q[i]);
      x1 = Math.max(x1, q[i]);
      y0 = Math.min(y0, q[i + 1]);
      y1 = Math.max(y1, q[i + 1]);
    }
  }
  return [x0, y0, x1, y1];
}
