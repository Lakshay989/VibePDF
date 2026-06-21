// SPEC: P3-ANN-006 (P3.C3a) — the stamp armed for placement. Picking a stamp in
// the palette arms it here; the per-page StampLayer reads it and drops it on the
// next click. Distinct from `tool-store` so the toolbar palette and the distant
// page layers stay in sync without prop-drilling.

import { create } from "zustand";

import type { StampSpec } from "@/tools/stamp/stamps";

interface StampState {
  /** The stamp armed for placement, or `null` when none is chosen. */
  armed: StampSpec | null;
  arm: (spec: StampSpec | null) => void;
}

export const useStampStore = create<StampState>((set) => ({
  armed: null,
  arm: (spec) => set({ armed: spec }),
}));
