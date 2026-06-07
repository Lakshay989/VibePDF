// SPEC: P2-PAGE-002 — the drag-reorder permutation helper.

import { describe, expect, it } from "vitest";

import { isPermutation, movePage } from "@/tools/reorder/compute-reorder";

describe("movePage", () => {
  it("moves a page later (front to back)", () => {
    expect(movePage(3, 0, 2)).toEqual([1, 2, 0]);
  });

  it("moves a page earlier (back to front)", () => {
    expect(movePage(3, 2, 0)).toEqual([2, 0, 1]);
  });

  it("moves a middle page", () => {
    expect(movePage(4, 1, 3)).toEqual([0, 2, 3, 1]);
  });

  it("is the identity when from === to", () => {
    expect(movePage(4, 2, 2)).toEqual([0, 1, 2, 3]);
  });

  it("is the identity for out-of-range indices", () => {
    expect(movePage(3, 5, 0)).toEqual([0, 1, 2]);
    expect(movePage(3, 0, -1)).toEqual([0, 1, 2]);
  });

  it("always returns a valid permutation", () => {
    for (let to = 0; to < 5; to += 1) {
      expect(isPermutation(movePage(5, 0, to))).toBe(true);
    }
  });
});

describe("isPermutation", () => {
  it("accepts a bijection of 0..n", () => {
    expect(isPermutation([2, 0, 1])).toBe(true);
  });
  it("rejects duplicates", () => {
    expect(isPermutation([0, 0, 1])).toBe(false);
  });
  it("rejects out-of-range entries", () => {
    expect(isPermutation([0, 1, 9])).toBe(false);
  });
});
