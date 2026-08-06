// SPEC: P5-FORM-006c (P5.B3) — the tab-order list permutation rules.

import { describe, expect, it } from "vitest";

import { moveDown, moveItem, moveUp } from "@/tools/form-author/tab-order";

const L = ["a", "b", "c", "d"];

describe("moveItem", () => {
  it("moves an item later", () => {
    expect(moveItem(L, 0, 2)).toEqual(["b", "c", "a", "d"]);
  });

  it("moves an item earlier", () => {
    expect(moveItem(L, 3, 1)).toEqual(["a", "d", "b", "c"]);
  });

  it("is a no-op when from === to", () => {
    expect(moveItem(L, 2, 2)).toEqual(L);
  });

  it("clamps out-of-range indices", () => {
    expect(moveItem(L, 0, 99)).toEqual(["b", "c", "d", "a"]);
    expect(moveItem(L, 99, 0)).toEqual(["d", "a", "b", "c"]);
  });

  it("does not mutate the input", () => {
    const src = [...L];
    moveItem(src, 0, 3);
    expect(src).toEqual(L);
  });

  it("handles an empty list", () => {
    expect(moveItem([], 0, 1)).toEqual([]);
  });
});

describe("moveUp / moveDown", () => {
  it("moves one slot", () => {
    expect(moveUp(L, 2)).toEqual(["a", "c", "b", "d"]);
    expect(moveDown(L, 1)).toEqual(["a", "c", "b", "d"]);
  });

  it("no-ops at the ends", () => {
    expect(moveUp(L, 0)).toEqual(L);
    expect(moveDown(L, L.length - 1)).toEqual(L);
  });
});
