// Edit-preview pipeline — the per-document "something changed" signal.
// The main view and thumbnails subscribe and reload when a doc's epoch
// changes. Tested at the store-action level.

import { beforeEach, describe, expect, it } from "vitest";

import { useEditEpochStore } from "@/state/edit-epoch-store";

beforeEach(() => {
  useEditEpochStore.setState({ byDoc: {} });
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
});
