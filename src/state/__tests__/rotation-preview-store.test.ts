// SPEC: P2-PAGE-001 (rotate fast-path) — the cosmetic per-page rotation
// the main view previews while PDFium holds the real /Rotate. Reset on any
// document reload. Tested at the store-action level.

import { beforeEach, describe, expect, it } from "vitest";

import { useRotationPreviewStore } from "@/state/rotation-preview-store";

beforeEach(() => {
  useRotationPreviewStore.setState({ byDoc: {} });
});

describe("rotation-preview-store", () => {
  it("accumulates rotation per page, normalised to [0,360)", () => {
    const { rotate } = useRotationPreviewStore.getState();
    rotate("d1", 0, 90);
    rotate("d1", 0, 90);
    expect(useRotationPreviewStore.getState().byDoc["d1"][0]).toBe(180);
    rotate("d1", 0, 180); // 180 + 180 = 360 → 0
    expect(useRotationPreviewStore.getState().byDoc["d1"][0]).toBe(0);
  });

  it("normalises negative (left) rotations", () => {
    useRotationPreviewStore.getState().rotate("d1", 2, -90);
    expect(useRotationPreviewStore.getState().byDoc["d1"][2]).toBe(270);
  });

  it("keeps pages and documents independent", () => {
    const { rotate } = useRotationPreviewStore.getState();
    rotate("d1", 0, 90);
    rotate("d1", 1, 180);
    rotate("d2", 0, 270);
    const { byDoc } = useRotationPreviewStore.getState();
    expect(byDoc["d1"]).toEqual({ 0: 90, 1: 180 });
    expect(byDoc["d2"]).toEqual({ 0: 270 });
  });

  it("resetDoc clears a document's rotations (e.g. on reload)", () => {
    const { rotate, resetDoc } = useRotationPreviewStore.getState();
    rotate("d1", 0, 90);
    rotate("d2", 0, 90);
    resetDoc("d1");
    expect("d1" in useRotationPreviewStore.getState().byDoc).toBe(false);
    expect(useRotationPreviewStore.getState().byDoc["d2"]).toEqual({ 0: 90 });
  });
});
