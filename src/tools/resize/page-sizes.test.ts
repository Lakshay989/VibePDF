import { describe, expect, it } from "vitest";

import {
  findStandardSize,
  isValidDimension,
  matchStandardSize,
  MAX_DIMENSION_PT,
  STANDARD_SIZES,
} from "./page-sizes";

describe("page-sizes", () => {
  it("maps standard names to the correct points", () => {
    expect(findStandardSize("Letter")).toEqual({ name: "Letter", width: 612, height: 792 });
    expect(findStandardSize("A4")).toEqual({ name: "A4", width: 595.28, height: 841.89 });
    expect(findStandardSize("Legal")?.height).toBe(1008);
  });

  it("returns undefined for an unknown / custom name", () => {
    expect(findStandardSize("Custom")).toBeUndefined();
    expect(findStandardSize("B5")).toBeUndefined();
  });

  it("offers the documented presets", () => {
    expect(STANDARD_SIZES.map((s) => s.name)).toEqual([
      "Letter",
      "Legal",
      "Tabloid",
      "A3",
      "A4",
      "A5",
    ]);
  });

  it("matches a current page size back to its preset (so the dialog reflects the page)", () => {
    expect(matchStandardSize(612, 792)).toBe("Letter");
    expect(matchStandardSize(595.28, 841.89)).toBe("A4");
    // Within tolerance (PDF.js view values can be fractionally off).
    expect(matchStandardSize(611.7, 792.3)).toBe("Letter");
    // A non-standard size → no preset (dialog falls back to Custom).
    expect(matchStandardSize(500, 700)).toBeUndefined();
  });

  it("validates dimensions: positive, finite, within the limit", () => {
    expect(isValidDimension(612)).toBe(true);
    expect(isValidDimension(MAX_DIMENSION_PT)).toBe(true);
    expect(isValidDimension(0)).toBe(false);
    expect(isValidDimension(-10)).toBe(false);
    expect(isValidDimension(Number.NaN)).toBe(false);
    expect(isValidDimension(Number.POSITIVE_INFINITY)).toBe(false);
    expect(isValidDimension(MAX_DIMENSION_PT + 1)).toBe(false);
  });
});
