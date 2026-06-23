// SPEC: P3-ANN-006 — the built-in stamp library + the click-to-place geometry.

import { describe, expect, it } from "vitest";

import {
  BUILTIN_STAMPS,
  customStamp,
  imageStamp,
  STAMP_HEIGHT,
  STAMP_WIDTH,
  stampRectAt,
} from "@/tools/stamp/stamps";

describe("BUILTIN_STAMPS", () => {
  it("every stamp has a name, label, and colour, with unique names", () => {
    expect(BUILTIN_STAMPS.length).toBeGreaterThan(0);
    for (const s of BUILTIN_STAMPS) {
      expect(s.name).toMatch(/^\w+$/); // a clean /Name token
      expect(s.label.length).toBeGreaterThan(0);
      expect(s.color).toMatch(/^#[0-9a-f]{6}$/i);
    }
    const names = BUILTIN_STAMPS.map((s) => s.name);
    expect(new Set(names).size).toBe(names.length);
  });
});

describe("customStamp", () => {
  it("carries the text as its label with a default colour", () => {
    const s = customStamp("Paid");
    expect(s.kind).toBe("text");
    expect(s.label).toBe("Paid");
    expect(s.color).toMatch(/^#[0-9a-f]{6}$/i);
  });
});

describe("imageStamp", () => {
  it("carries the path and derives a name from the file", () => {
    const s = imageStamp("/Users/me/Signatures/sig.png");
    expect(s.kind).toBe("image");
    expect(s.imagePath).toBe("/Users/me/Signatures/sig.png");
    expect(s.name).toBe("sig.png");
    expect(s.label).toBeUndefined();
  });

  it("keeps an optional overlay label", () => {
    const s = imageStamp("C:\\logos\\logo.png", "DRAFT");
    expect(s.label).toBe("DRAFT");
    expect(s.name).toBe("logo.png");
  });
});

describe("stampRectAt", () => {
  it("centres the default box on the point", () => {
    const [x0, y0, x1, y1] = stampRectAt(300, 400, 612, 792);
    expect(x1 - x0).toBeCloseTo(STAMP_WIDTH);
    expect(y1 - y0).toBeCloseTo(STAMP_HEIGHT);
    expect((x0 + x1) / 2).toBeCloseTo(300);
    expect((y0 + y1) / 2).toBeCloseTo(400);
  });

  it("clamps the box inside the page near an edge", () => {
    const [x0, y0, x1, y1] = stampRectAt(5, 5, 612, 792);
    expect(x0).toBe(0);
    expect(y0).toBe(0);
    expect(x1).toBeCloseTo(STAMP_WIDTH);
    expect(y1).toBeCloseTo(STAMP_HEIGHT);
  });
});
