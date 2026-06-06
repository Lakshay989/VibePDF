// SPEC: P2-PAGE-007 — the split-points parser for the "at specific pages"
// split mode.

import { describe, expect, it } from "vitest";

import { parseSplitPoints } from "@/tools/split/split-points";

describe("parseSplitPoints", () => {
  it("parses a comma list to sorted, unique boundaries", () => {
    expect(parseSplitPoints("4, 7, 10", 12)).toEqual({ boundaries: [4, 7, 10] });
  });

  it("de-duplicates and sorts", () => {
    expect(parseSplitPoints("7, 4, 7", 12)).toEqual({ boundaries: [4, 7] });
  });

  it("rejects page 1 (a split before the first page is meaningless)", () => {
    expect(parseSplitPoints("1", 12)).toEqual({
      error: expect.stringContaining("between 2 and 12"),
    });
  });

  it("rejects a boundary past the last page", () => {
    expect(parseSplitPoints("13", 12)).toEqual({
      error: expect.stringContaining("between 2 and 12"),
    });
  });

  it("rejects non-numeric tokens", () => {
    expect(parseSplitPoints("4, x", 12)).toEqual({
      error: expect.stringContaining("isn't a page number"),
    });
  });

  it("rejects an empty input", () => {
    expect(parseSplitPoints("   ", 12)).toEqual({
      error: expect.stringContaining("at least one"),
    });
  });
});
