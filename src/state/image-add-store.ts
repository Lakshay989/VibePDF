// SPEC: P4-EDIT-005 (P4.C1) — the image file armed for placement. The Add Image
// toolbar button picks a file and arms its path here; the per-page ImageAddLayer
// reads it and embeds it into the box the user drags. Kept separate from
// `tool-store` so the toolbar and the distant page layers stay in sync.

import { create } from "zustand";

interface ImageAddState {
  /** Absolute path of the image to place, or `null` when none is armed. */
  path: string | null;
  arm: (path: string | null) => void;
}

export const useImageAddStore = create<ImageAddState>((set) => ({
  path: null,
  arm: (path) => set({ path }),
}));
