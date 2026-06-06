import { create } from "zustand";

import type { DocumentId } from "@/ipc/pdf";

// Stable empty map so the selector doesn't churn on a miss.
const EMPTY: Readonly<Record<number, number>> = Object.freeze({});

const norm = (deg: number) => ((deg % 360) + 360) % 360;

interface RotationPreviewState {
  // Cosmetic rotation in degrees (0/90/180/270), per 0-based page, applied
  // on top of the loaded document's baked-in rotation. PDFium (the actor)
  // holds the *real* /Rotate; this mirrors it so the main view can show a
  // rotation instantly, without re-parsing the whole document.
  //
  // It is reset to {} whenever the document is (re)loaded — the reloaded
  // bytes already carry the real rotation, so the cosmetic delta starts
  // over at 0. As long as every rotate updates both PDFium and this map by
  // the same amount, the preview and the persisted state never drift.
  byDoc: Record<DocumentId, Record<number, number>>;
  rotate: (id: DocumentId, page: number, deltaDegrees: number) => void;
  resetDoc: (id: DocumentId) => void;
}

export const useRotationPreviewStore = create<RotationPreviewState>((set) => ({
  byDoc: {},
  rotate: (id, page, delta) =>
    set((s) => {
      const cur = s.byDoc[id] ?? {};
      const next = norm((cur[page] ?? 0) + delta);
      return { byDoc: { ...s.byDoc, [id]: { ...cur, [page]: next } } };
    }),
  resetDoc: (id) =>
    set((s) => {
      if (!(id in s.byDoc)) return s;
      const next = { ...s.byDoc };
      delete next[id];
      return { byDoc: next };
    }),
}));

/** Reactive selector: a document's cosmetic per-page rotations. */
export function useDocRotations(
  id: DocumentId | undefined,
): Readonly<Record<number, number>> {
  return useRotationPreviewStore((s) => (id ? (s.byDoc[id] ?? EMPTY) : EMPTY));
}
