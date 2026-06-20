// SPEC: P3-ANN-013 (edit-in-place) — a cross-cutting channel for "re-open the
// editor on this free-text annotation".
//
// The sidebar (top of the tree) reads a free-text box's state and posts an edit
// request here; the per-page `FreeTextLayer` (deep in the page list) picks it up
// and opens its editor pre-filled. Like `annotation-selection-store`, this just
// carries the request — one consumer claims it and clears it.

import { create } from "zustand";

import type { FreeTextData } from "@/ipc/freetext";

export interface FreeTextEditRequest {
  /** The annotation's `/NM` (the update target). */
  nm: string;
  /** 0-based page the box lives on (so the right `FreeTextLayer` claims it). */
  page: number;
  /** Its current text + style, for the pre-filled editor. */
  data: FreeTextData;
}

interface AnnotationEditState {
  editing: FreeTextEditRequest | null;
  requestEdit: (req: FreeTextEditRequest) => void;
  clearEdit: () => void;
}

export const useAnnotationEditStore = create<AnnotationEditState>((set) => ({
  editing: null,
  requestEdit: (req) => set({ editing: req }),
  clearEdit: () => set({ editing: null }),
}));
