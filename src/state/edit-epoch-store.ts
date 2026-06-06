import { create } from "zustand";

import type { DocumentId } from "@/ipc/pdf";

interface EditEpochState {
  // Monotonic per-document edit counter — the "reload the main view" signal.
  // Bumped by edits that need a full reload (delete / undo / redo); the main
  // view and thumbnails subscribe and reload when it changes. The rotate
  // fast-path does NOT bump it (it previews cosmetically).
  byDoc: Record<DocumentId, number>;
  // Documents that have *any* unsaved in-memory edit. This decides the main
  // view's byte source: a pristine document loads from disk (cheap); an
  // edited one must load from the actor's live bytes (which carry the edits)
  // — including the rotate fast-path, which doesn't bump the epoch. Once set
  // it stays set for the document's lifetime (after a save the actor bytes
  // equal disk anyway, so loading from the actor is still correct).
  edited: Record<DocumentId, true>;
  bumpEpoch: (id: DocumentId) => void;
  markEdited: (id: DocumentId) => void;
}

export const useEditEpochStore = create<EditEpochState>((set) => ({
  byDoc: {},
  edited: {},
  bumpEpoch: (id) =>
    set((s) => ({
      byDoc: { ...s.byDoc, [id]: (s.byDoc[id] ?? 0) + 1 },
      // An epoch bump is always an edit.
      edited: s.edited[id] ? s.edited : { ...s.edited, [id]: true },
    })),
  markEdited: (id) =>
    set((s) => (s.edited[id] ? s : { edited: { ...s.edited, [id]: true } })),
}));

/** Reactive selector: a document's edit epoch (0 before any reload-edit). */
export function useDocEpoch(id: DocumentId | undefined): number {
  return useEditEpochStore((s) => (id ? (s.byDoc[id] ?? 0) : 0));
}

/**
 * Non-reactive read: has this document any unsaved in-memory edit? Used by
 * the loader to choose disk (pristine) vs the actor's live bytes (edited).
 * Read imperatively so it doesn't re-trigger the load effect when it flips.
 */
export function isDocEdited(id: DocumentId): boolean {
  return useEditEpochStore.getState().edited[id] ?? false;
}
