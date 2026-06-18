// SPEC: P3-ANN-003 — pure helpers for the free-text tool: the font catalog, the
// CSS font mapping, and the screen-rect math (drag → box, with a default box for
// a click).

import { describe, expect, it } from "vitest";

import {
  cssFontFamily,
  DEFAULT_BOX_PX,
  FONT_FAMILIES,
  normalizeScreenRect,
  withDefaultSize,
} from "@/tools/free-text/free-text";

describe("free-text helpers", () => {
  it("offers the three base-14 families", () => {
    expect(FONT_FAMILIES).toEqual(["Helvetica", "Times", "Courier"]);
  });

  it("maps each family to a CSS stack", () => {
    expect(cssFontFamily("Helvetica")).toMatch(/Helvetica/);
    expect(cssFontFamily("Times")).toMatch(/Times/);
    expect(cssFontFamily("Courier")).toMatch(/Courier/);
  });

  it("normalizes two points into a positive rect regardless of drag direction", () => {
    const a = normalizeScreenRect({ x: 100, y: 80 }, { x: 30, y: 200 });
    expect(a).toEqual({ left: 30, top: 80, width: 70, height: 120 });
  });

  it("keeps a real drag but expands a click to the default box", () => {
    const drag = normalizeScreenRect({ x: 10, y: 10 }, { x: 110, y: 50 });
    expect(withDefaultSize(drag)).toEqual(drag);

    const click = normalizeScreenRect({ x: 40, y: 40 }, { x: 42, y: 41 });
    expect(withDefaultSize(click)).toEqual({
      left: 40,
      top: 40,
      width: DEFAULT_BOX_PX.width,
      height: DEFAULT_BOX_PX.height,
    });
  });
});
