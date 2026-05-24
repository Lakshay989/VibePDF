import { create } from "zustand";

export type FitMode = "actual" | "fit-page" | "fit-width" | "fit-height";

// SPEC: P1-VIEW-006 — clamp to the spec'd zoom range.
export const MIN_ZOOM = 0.25;
export const MAX_ZOOM = 16;

interface ViewState {
  zoom: number;
  fitMode: FitMode | null;
  showThumbnails: boolean;
  showOutline: boolean;
  setZoom: (z: number) => void;
  setFitMode: (m: FitMode) => void;
  setView: (next: { zoom: number; fitMode: FitMode | null }) => void;
  toggleThumbnails: () => void;
  toggleOutline: () => void;
}

const clampZoom = (z: number) => Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, z));

export const useViewStore = create<ViewState>((set) => ({
  zoom: 1,
  fitMode: "fit-page",
  showThumbnails: true,
  showOutline: false,
  setZoom: (z) => set({ zoom: clampZoom(z), fitMode: null }),
  setFitMode: (m) => set({ fitMode: m }),
  setView: ({ zoom, fitMode }) => set({ zoom: clampZoom(zoom), fitMode }),
  toggleThumbnails: () => set((s) => ({ showThumbnails: !s.showThumbnails })),
  toggleOutline: () => set((s) => ({ showOutline: !s.showOutline })),
}));
