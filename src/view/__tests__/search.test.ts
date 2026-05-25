import { describe, expect, it } from "vitest";

import { findRanges, totalMatches } from "@/view/search";

const opts = (caseSensitive = false, wholeWord = false) => ({
  caseSensitive,
  wholeWord,
});

describe("findRanges", () => {
  it("returns [] for empty query", () => {
    expect(findRanges("hello world", "", opts())).toEqual([]);
  });

  it("returns [] for empty text", () => {
    expect(findRanges("", "the", opts())).toEqual([]);
  });

  it("finds a single case-insensitive match", () => {
    expect(findRanges("The quick brown fox", "the", opts(false))).toEqual([
      { start: 0, end: 3 },
    ]);
  });

  it("rejects case-mismatched query when case-sensitive", () => {
    expect(findRanges("THE QUICK", "the", opts(true))).toEqual([]);
    expect(findRanges("The quick", "The", opts(true))).toEqual([
      { start: 0, end: 3 },
    ]);
  });

  it("finds multiple non-overlapping matches", () => {
    expect(
      findRanges("the cat and the dog and the bird", "the", opts(false)),
    ).toEqual([
      { start: 0, end: 3 },
      { start: 12, end: 15 },
      { start: 24, end: 27 },
    ]);
  });

  it("whole-word matches reject substrings", () => {
    expect(findRanges("then there", "the", opts(false, true))).toEqual([]);
  });

  it("whole-word matches accept standalone occurrences", () => {
    expect(
      findRanges("the cat sat and the dog ran", "the", opts(false, true)),
    ).toEqual([
      { start: 0, end: 3 },
      { start: 16, end: 19 },
    ]);
  });

  it("treats regex metacharacters as literals", () => {
    // "a.b" must match only literal "a.b", NOT "axb" (regex behavior).
    expect(findRanges("axb a.b axb", "a.b", opts())).toEqual([
      { start: 4, end: 7 },
    ]);
  });

  it("handles overlapping potential matches deterministically", () => {
    // "aa" in "aaaa" → matches at 0 and 2 (non-overlapping, left-to-right).
    expect(findRanges("aaaa", "aa", opts())).toEqual([
      { start: 0, end: 2 },
      { start: 2, end: 4 },
    ]);
  });

  it("returns matches ordered by position", () => {
    const ranges = findRanges("xx yy xx yy xx", "xx", opts());
    const starts = ranges.map((r) => r.start);
    expect(starts).toEqual([...starts].sort((a, b) => a - b));
  });
});

describe("totalMatches", () => {
  it("sums ranges across pages", () => {
    expect(
      totalMatches([
        { pageNumber: 1, ranges: [{ start: 0, end: 1 }, { start: 5, end: 6 }] },
        { pageNumber: 3, ranges: [{ start: 0, end: 1 }] },
      ]),
    ).toBe(3);
  });

  it("returns 0 for []", () => {
    expect(totalMatches([])).toBe(0);
  });
});
