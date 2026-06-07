// SPEC: P2-PAGE-008 — pure reorder helpers for the merge dialog's file list.

import { describe, expect, it } from "vitest";

import { moveDown, moveUp, removeAt } from "@/tools/merge/reorder";

describe("moveUp", () => {
  it("swaps an item with the one before it", () => {
    expect(moveUp(["a", "b", "c"], 1)).toEqual(["b", "a", "c"]);
  });
  it("is a no-op at the top", () => {
    expect(moveUp(["a", "b"], 0)).toEqual(["a", "b"]);
  });
  it("does not mutate the input", () => {
    const input = ["a", "b"];
    moveUp(input, 1);
    expect(input).toEqual(["a", "b"]);
  });
});

describe("moveDown", () => {
  it("swaps an item with the one after it", () => {
    expect(moveDown(["a", "b", "c"], 1)).toEqual(["a", "c", "b"]);
  });
  it("is a no-op at the bottom", () => {
    expect(moveDown(["a", "b"], 1)).toEqual(["a", "b"]);
  });
});

describe("removeAt", () => {
  it("removes the item at the index", () => {
    expect(removeAt(["a", "b", "c"], 1)).toEqual(["a", "c"]);
  });
  it("returns a copy unchanged for an out-of-range index", () => {
    expect(removeAt(["a", "b"], 9)).toEqual(["a", "b"]);
  });
});
