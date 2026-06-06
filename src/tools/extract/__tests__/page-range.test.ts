// SPEC: P2-PAGE-006 — the page-range parser for the extract dialog.

import { describe, expect, it } from "vitest";

import { parsePageRange } from "@/tools/extract/page-range";

describe("parsePageRange", () => {
  it("parses a mixed range to sorted, unique, 0-based indices", () => {
    expect(parsePageRange("2-3, 5, 8-10", 10)).toEqual({
      pages: [1, 2, 4, 7, 8, 9],
    });
  });

  it("de-duplicates overlapping ranges", () => {
    expect(parsePageRange("1-3, 2-4", 10)).toEqual({ pages: [0, 1, 2, 3] });
  });

  it("accepts a reversed range", () => {
    expect(parsePageRange("3-1", 10)).toEqual({ pages: [0, 1, 2] });
  });

  it("rejects pages outside 1..pageCount", () => {
    expect(parsePageRange("11", 10)).toEqual({
      error: expect.stringContaining("between 1 and 10"),
    });
    expect(parsePageRange("0", 10)).toEqual({
      error: expect.stringContaining("between 1 and 10"),
    });
  });

  it("rejects non-numeric tokens", () => {
    expect("error" in parsePageRange("abc", 10)).toBe(true);
  });

  it("errors on empty input", () => {
    expect(parsePageRange("   ", 10)).toEqual({
      error: expect.stringContaining("at least one"),
    });
  });
});
