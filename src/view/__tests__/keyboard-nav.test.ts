import { describe, expect, it } from "vitest";

import { keyToIntent } from "@/view/keyboard-nav";

const noInput = { inputFocused: false };
const withInput = { inputFocused: true };

describe("keyToIntent", () => {
  it("PageDown → next page", () => {
    expect(keyToIntent({ key: "PageDown" }, noInput)).toEqual({
      kind: "page-delta",
      delta: 1,
    });
  });

  it("PageUp → previous page", () => {
    expect(keyToIntent({ key: "PageUp" }, noInput)).toEqual({
      kind: "page-delta",
      delta: -1,
    });
  });

  it("Home → first page", () => {
    expect(keyToIntent({ key: "Home" }, noInput)).toEqual({
      kind: "page-target",
      page: "first",
    });
  });

  it("End → last page", () => {
    expect(keyToIntent({ key: "End" }, noInput)).toEqual({
      kind: "page-target",
      page: "last",
    });
  });

  it("ArrowDown / ArrowUp → line scroll when no input is focused", () => {
    expect(keyToIntent({ key: "ArrowDown" }, noInput)).toEqual({
      kind: "line-delta",
      delta: 40,
    });
    expect(keyToIntent({ key: "ArrowUp" }, noInput)).toEqual({
      kind: "line-delta",
      delta: -40,
    });
  });

  it("ArrowDown / ArrowUp → ignored when an input is focused", () => {
    expect(keyToIntent({ key: "ArrowDown" }, withInput)).toBeNull();
    expect(keyToIntent({ key: "ArrowUp" }, withInput)).toBeNull();
  });

  it("modifier combos are never absorbed (belong to app-level shortcuts)", () => {
    expect(keyToIntent({ key: "PageDown", metaKey: true }, noInput)).toBeNull();
    expect(keyToIntent({ key: "Home", ctrlKey: true }, noInput)).toBeNull();
    expect(keyToIntent({ key: "ArrowDown", altKey: true }, noInput)).toBeNull();
  });

  it("returns null for unmapped keys", () => {
    expect(keyToIntent({ key: "a" }, noInput)).toBeNull();
    expect(keyToIntent({ key: "Enter" }, noInput)).toBeNull();
    expect(keyToIntent({ key: "Escape" }, noInput)).toBeNull();
    expect(keyToIntent({ key: " " }, noInput)).toBeNull();
  });
});
