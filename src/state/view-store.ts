import { create } from "zustand";

export type FitMode = "actual" | "fit-page" | "fit-width" | "fit-height";

interface ViewState {
  zoom: number;
  fitMode: FitMode | null;
  showThumbnails: boolean;
  showOutline: boolean;
  setZoom: (z: number) => void;
  setFitMode: (m: FitMode) => void;
  toggleThumbnails: () => void;
  toggleOutline: () => void;
}

// SPEC: P1-VIEW-006 — clamp to the spec'd zoom range. Persistence per
// document is handled by a separate IndexedDB layer (Phase 1 follow-up).
const MIN_ZOOM = 0.25;
const MAX_ZOOM = 16;

export const useViewStore = create<ViewState>((set) => ({
  zoom: 1,
  fitMode: "fit-page",
  showThumbnails: true,
  showOutline: false,
  setZoom: (z) => set({ zoom: Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, z)), fitMode: null }),
  setFitMode: (m) => set({ fitMode: m }),
  toggleThumbnails: () => set((s) => ({ showThumbnails: !s.showThumbnails })),
  toggleOutline: () => set((s) => ({ showOutline: !s.showOutline })),
}));
