// Edit-preview pipeline — the per-document "something changed" signal.
// The main view and thumbnails subscribe and reload when a doc's epoch
// changes. Tested at the store-action level.

import { beforeEach, describe, expect, it } from "vitest";

import { isDocEdited, useEditEpochStore } from "@/state/edit-epoch-store";

beforeEach(() => {
  useEditEpochStore.setState({ byDoc: {}, edited: {} });
});

describe("edit-epoch-store", () => {
  it("is absent (treated as 0) until the first bump, then increments", () => {
    expect(useEditEpochStore.getState().byDoc["d1"]).toBeUndefined();
    useEditEpochStore.getState().bumpEpoch("d1");
    expect(useEditEpochStore.getState().byDoc["d1"]).toBe(1);
    useEditEpochStore.getState().bumpEpoch("d1");
    expect(useEditEpochStore.getState().byDoc["d1"]).toBe(2);
  });

  it("keeps documents independent", () => {
    const { bumpEpoch } = useEditEpochStore.getState();
    bumpEpoch("d1");
    bumpEpoch("d1");
    bumpEpoch("d2");
    expect(useEditEpochStore.getState().byDoc["d1"]).toBe(2);
    expect(useEditEpochStore.getState().byDoc["d2"]).toBe(1);
  });

  it("tracks the edited flag: false until marked or bumped", () => {
    expect(isDocEdited("d1")).toBe(false);
    // A rotate marks edited *without* bumping the epoch.
    useEditEpochStore.getState().markEdited("d1");
    expect(isDocEdited("d1")).toBe(true);
    expect(useEditEpochStore.getState().byDoc["d1"]).toBeUndefined();
  });

  it("bumpEpoch also marks the doc edited (delete/undo/redo)", () => {
    expect(isDocEdited("d2")).toBe(false);
    useEditEpochStore.getState().bumpEpoch("d2");
    expect(isDocEdited("d2")).toBe(true);
    expect(isDocEdited("d1")).toBe(false); // independent
  });
});
