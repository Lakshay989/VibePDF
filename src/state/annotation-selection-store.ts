// SPEC: P3-ANN-008 (P3.D1) — the annotation selected in the sidebar.
//
// A cross-cutting channel (like tool-store): the sidebar sets the selection on
// click, and the per-page `SelectionHighlightLayer` reads it to draw a dashed
// box at the annotation's bounds. Read-only selection — it carries the
// annotation's geometry so the highlight needs no separate lookup.

import { create } from "zustand";

import type { AnnotationInfo } from "@/ipc/annotations";

interface AnnotationSelectionState {
  selected: AnnotationInfo | null;
  select: (info: AnnotationInfo | null) => void;
}

export const useAnnotationSelectionStore = create<AnnotationSelectionState>((set) => ({
  selected: null,
  select: (info) => set({ selected: info }),
}));
