import { useEffect, useRef, useState } from "react";
import { create } from "zustand";

import type { DocumentId } from "@/ipc/pdf";

interface EditEpochState {
  // Raw per-document edit counter — bumped by EVERY edit. Drives the sidebar +
  // the optimistic-overlay bookkeeping, which must reflect an edit at once.
  byDoc: Record<DocumentId, number>;
  // Bake (reload) counter — bumped only by edits that need a true re-render of
  // the canvas. The main view reloads/re-rasterizes on THIS, so the add-annotation
  // tools (whose overlay already shows the edit) can bump only `byDoc` and skip
  // the expensive full-document reload until a bake is actually required.
  bakeByDoc: Record<DocumentId, number>;
  // Documents with soft (add-annotation) edits not yet flushed to the canvas.
  // Set by a soft bump, cleared by any bake (hard edit or the idle backstop). The
  // idle backstop keys off THIS, not a raw-vs-bake comparison — raw and bake are
  // independent counters (every edit vs every bake), so comparing them is wrong
  // and would re-fire the backstop forever.
  pendingBake: Record<DocumentId, true>;
  // Documents that have *any* unsaved in-memory edit. This decides the main
  // view's byte source: a pristine document loads from disk (cheap); an
  // edited one must load from the actor's live bytes (which carry the edits)
  // — including the rotate fast-path, which doesn't bump the epoch. Once set
  // it stays set for the document's lifetime (after a save the actor bytes
  // equal disk anyway, so loading from the actor is still correct).
  edited: Record<DocumentId, true>;
  /** Hard edit (undo/redo, delete, page ops, dialogs, …): bumps raw AND bake, so
   *  the main view reloads. Returns the bake epoch that will render this edit — the
   *  value an optimistic overlay should `tie` to. */
  bumpEpoch: (id: DocumentId) => number;
  /** Soft edit (an add-annotation tool whose overlay already shows it): bumps only
   *  the raw epoch, so the sidebar updates but the main view does NOT reload.
   *  Returns the *next* bake epoch — the one that will eventually bake this edit,
   *  so its overlay survives until then. */
  bumpEpochSoft: (id: DocumentId) => number;
  /** Force a bake (the idle backstop: flush accumulated soft edits to the canvas). */
  bumpBake: (id: DocumentId) => void;
  markEdited: (id: DocumentId) => void;
}

export const useEditEpochStore = create<EditEpochState>((set, get) => ({
  byDoc: {},
  bakeByDoc: {},
  pendingBake: {},
  edited: {},
  bumpEpoch: (id) => {
    const bake = (get().bakeByDoc[id] ?? 0) + 1;
    set((s) => {
      const pendingBake = { ...s.pendingBake };
      delete pendingBake[id]; // a hard edit bakes everything; nothing soft left to flush
      return {
        byDoc: { ...s.byDoc, [id]: (s.byDoc[id] ?? 0) + 1 },
        bakeByDoc: { ...s.bakeByDoc, [id]: bake },
        pendingBake,
        edited: s.edited[id] ? s.edited : { ...s.edited, [id]: true },
      };
    });
    return bake;
  },
  bumpEpochSoft: (id) => {
    // The bake that will include this soft edit is the *next* one; the overlay
    // ties to it so it isn't pruned until that bake paints.
    const nextBake = (get().bakeByDoc[id] ?? 0) + 1;
    set((s) => ({
      byDoc: { ...s.byDoc, [id]: (s.byDoc[id] ?? 0) + 1 },
      pendingBake: { ...s.pendingBake, [id]: true },
      edited: s.edited[id] ? s.edited : { ...s.edited, [id]: true },
    }));
    return nextBake;
  },
  bumpBake: (id) =>
    set((s) => {
      const pendingBake = { ...s.pendingBake };
      delete pendingBake[id]; // one bake flushes all soft edits since the last one
      return { bakeByDoc: { ...s.bakeByDoc, [id]: (s.bakeByDoc[id] ?? 0) + 1 }, pendingBake };
    }),
  markEdited: (id) =>
    set((s) => (s.edited[id] ? s : { edited: { ...s.edited, [id]: true } })),
}));

/** Reactive selector: a document's raw edit epoch (0 before any edit). */
export function useDocEpoch(id: DocumentId | undefined): number {
  return useEditEpochStore((s) => (id ? (s.byDoc[id] ?? 0) : 0));
}

/** Reactive selector: a document's bake (reload) epoch (0 before any bake). */
export function useBakeEpoch(id: DocumentId | undefined): number {
  return useEditEpochStore((s) => (id ? (s.bakeByDoc[id] ?? 0) : 0));
}

/** True while a document has soft edits not yet flushed to the canvas — the idle
 *  backstop watches this and fires exactly one bake, then it clears. */
export function useHasPendingBake(id: DocumentId | undefined): boolean {
  return useEditEpochStore((s) => (id ? (s.pendingBake[id] ?? false) : false));
}

/**
 * Debounce a per-document counter, holding it steady until it stops changing for
 * `delayMs`. On a document switch the value snaps immediately (the hosting
 * component isn't remounted, only its `id` prop changes) — otherwise the stale
 * held value would settle `delayMs` later and fire a second, spurious reload.
 */
function useDebouncedValue(value: number, id: DocumentId | undefined, delayMs: number): number {
  const [debounced, setDebounced] = useState(value);
  const lastId = useRef(id);
  if (lastId.current !== id) {
    lastId.current = id;
    if (debounced !== value) setDebounced(value);
  }
  useEffect(() => {
    if (value === debounced) return undefined;
    const timer = setTimeout(() => setDebounced(value), delayMs);
    return () => clearTimeout(timer);
  }, [value, debounced, delayMs]);
  return debounced;
}

/** The raw epoch, debounced. */
export function useDebouncedDocEpoch(id: DocumentId | undefined, delayMs: number): number {
  return useDebouncedValue(useDocEpoch(id), id, delayMs);
}

/**
 * The bake epoch, debounced. The main view reloads on this: it advances only on a
 * hard edit or the idle backstop, so a burst of soft (add-annotation) edits causes
 * no reload — their overlays carry the change until a bake is actually needed.
 */
export function useDebouncedBakeEpoch(id: DocumentId | undefined, delayMs: number): number {
  return useDebouncedValue(useBakeEpoch(id), id, delayMs);
}

/**
 * Non-reactive read: has this document any unsaved in-memory edit? Used by
 * the loader to choose disk (pristine) vs the actor's live bytes (edited).
 * Read imperatively so it doesn't re-trigger the load effect when it flips.
 */
export function isDocEdited(id: DocumentId): boolean {
  return useEditEpochStore.getState().edited[id] ?? false;
}
