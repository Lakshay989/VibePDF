import { describe, expect, it } from "vitest";

import type { PageGeometry } from "@/tools/_framework";

import { type LocalRect, quadsBounds, rectsToQuads, rectToQuad } from "../quads";

// Letter (612×792 pt) rendered at 2× CSS px/pt, no rotation.
const geo: PageGeometry = { page: 0, width: 612, height: 792, scale: 2, rotation: 0 };
const rect = (left: number, top: number, right: number, bottom: number): LocalRect => ({
  left,
  top,
  right,
  bottom,
});

describe("text-markup quads", () => {
  it("maps a line rect to a quad in /QuadPoints order (UL, UR, LL, LR)", () => {
    // CSS px (100,200)-(300,220) at 2× → PDF: x/2, y = 792 - y/2.
    const q = rectToQuad(rect(100, 200, 300, 220), geo);
    expect(q).toEqual([
      50, 692, // UL
      150, 692, // UR
      50, 682, // LL
      150, 682, // LR
    ]);
  });

  it("drops degenerate (zero-area) rects", () => {
    const quads = rectsToQuads([rect(10, 10, 10, 10), rect(0, 0, 20, 12)], geo);
    expect(quads).toHaveLength(1);
  });

  it("computes the bounding /Rect across quads", () => {
    const quads = rectsToQuads([rect(100, 200, 300, 220), rect(100, 230, 260, 250)], geo);
    const [x0, y0, x1, y1] = quadsBounds(quads);
    expect(x0).toBe(50);
    expect(x1).toBe(150);
    expect(y0).toBe(667); // bottom-most: 792 - 250/2 = 667
    expect(y1).toBe(692); // top-most: 792 - 200/2 = 692
  });

  it("respects page rotation (90° round-trips the corners)", () => {
    const rotated: PageGeometry = { ...geo, rotation: 90 };
    const q = rectToQuad(rect(100, 200, 300, 220), rotated);
    // screenToPdf(90): x = y/scale, y = x/scale → UL = (200/2, 100/2) = (100, 50).
    expect(q[0]).toBe(100);
    expect(q[1]).toBe(50);
  });
});
