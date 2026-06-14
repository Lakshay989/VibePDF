import { describe, expect, it } from "vitest";

import {
  normalizeRect,
  type PageGeometry,
  pdfRectFromPoints,
  pdfToScreen,
  screenToPdf,
} from "../coords";

// US-Letter (612×792 pt) rendered at 2× CSS px/pt.
const geo = (rotation: number): PageGeometry => ({
  page: 0,
  width: 612,
  height: 792,
  scale: 2,
  rotation,
});

describe("coords", () => {
  it("rotation 0: PDF bottom-left ↔ screen bottom-left, top-right ↔ top-right", () => {
    const g = geo(0);
    expect(pdfToScreen({ page: 0, x: 0, y: 0 }, g)).toEqual({ x: 0, y: 792 * 2 });
    expect(pdfToScreen({ page: 0, x: 612, y: 792 }, g)).toEqual({ x: 612 * 2, y: 0 });
  });

  it("90° CW maps PDF bottom-left to the screen top-left corner", () => {
    expect(pdfToScreen({ page: 0, x: 0, y: 0 }, geo(90))).toEqual({ x: 0, y: 0 });
  });

  it("round-trips screen → PDF → screen for every cardinal rotation", () => {
    const samples = [
      { x: 0, y: 0 },
      { x: 300, y: 400 },
      { x: 1000, y: 1200 },
    ];
    for (const rotation of [0, 90, 180, 270]) {
      const g = geo(rotation);
      for (const s of samples) {
        const back = pdfToScreen(screenToPdf(s, g), g);
        expect(back.x).toBeCloseTo(s.x, 6);
        expect(back.y).toBeCloseTo(s.y, 6);
      }
    }
  });

  it("normalizes rects regardless of corner order", () => {
    expect(
      pdfRectFromPoints({ page: 0, x: 50, y: 80 }, { page: 0, x: 10, y: 20 }),
    ).toEqual({ x0: 10, y0: 20, x1: 50, y1: 80 });
    expect(normalizeRect({ x0: 9, y0: 9, x1: 1, y1: 1 })).toEqual({
      x0: 1,
      y0: 1,
      x1: 9,
      y1: 9,
    });
  });
});
