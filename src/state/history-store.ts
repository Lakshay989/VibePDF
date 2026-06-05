import { create } from "zustand";

import type { HistoryState } from "@/ipc/history";
import type { DocumentId } from "@/ipc/pdf";

// Stable reference for "no history yet" so selectors don't re-render on
// every read (zustand compares with Object.is).
const EMPTY: HistoryState = { canUndo: false, canRedo: false };

interface HistoryStoreState {
  // Undo/redo availability keyed by documentId. The actor is the source
  // of truth; this mirror exists only to drive UI button state.
  byDoc: Record<DocumentId, HistoryState>;
  setHistory: (id: DocumentId, state: HistoryState) => void;
  clearHistory: (id: DocumentId) => void;
}

export const useHistoryStore = create<HistoryStoreState>((set) => ({
  byDoc: {},
  setHistory: (id, state) =>
    set((s) => ({ byDoc: { ...s.byDoc, [id]: state } })),
  clearHistory: (id) =>
    set((s) => {
      if (!(id in s.byDoc)) return s;
      const next = { ...s.byDoc };
      delete next[id];
      return { byDoc: next };
    }),
}));

/** Reactive selector: a document's current undo/redo availability. */
export function useDocHistory(id: DocumentId | undefined): HistoryState {
  return useHistoryStore((s) => (id ? (s.byDoc[id] ?? EMPTY) : EMPTY));
}
