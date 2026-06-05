import { create } from "zustand";

import type { DocumentId } from "@/ipc/pdf";

interface EditEpochState {
  // Monotonic per-document edit counter — the single "this document
  // changed" signal of the edit-preview pipeline. Bumped after every
  // successful edit / undo / redo; the main view and the thumbnails
  // subscribe and reload when their document's epoch changes. The actor
  // is the source of truth; this is only the trigger.
  byDoc: Record<DocumentId, number>;
  bumpEpoch: (id: DocumentId) => void;
}

export const useEditEpochStore = create<EditEpochState>((set) => ({
  byDoc: {},
  bumpEpoch: (id) =>
    set((s) => ({ byDoc: { ...s.byDoc, [id]: (s.byDoc[id] ?? 0) + 1 } })),
}));

/** Reactive selector: a document's edit epoch (0 before any edit). */
export function useDocEpoch(id: DocumentId | undefined): number {
  return useEditEpochStore((s) => (id ? (s.byDoc[id] ?? 0) : 0));
}
