// Optimistic edit preview (P4.HF29).
//
// An annotation edit is applied by the backend, then the whole document is
// reloaded (edit-epoch → PdfViewer) so PDF.js shows the *baked* result. On a
// large PDF that round-trip is seconds, and each tool clears its live preview
// the instant the gesture ends — so the committed shape visibly *disappears*
// until the reload lands. This store bridges that gap: the tool drops a
// lightweight render-spec here on commit (shown immediately by an overlay),
// and it is pruned once the reload that bakes it has painted.
//
// Lifecycle of one held edit:
//   1. `add(...)`  — on commit, before the IPC resolves. `epoch: null` = "show
//      me, don't prune yet" (we don't yet know which reload will contain it).
//   2. `tie(key, epoch)` — in the write's `.then`, after `bumpEpoch`, with the
//      *post-bump* epoch. That reload's bytes include this edit (the write
//      already resolved), so the edit is safe to drop once that epoch renders.
//   3. `markRendered(docId, epoch)` — PdfViewer calls this when a reload paints;
//      held edits tied to an epoch ≤ it are baked on the canvas, so we prune.
//   A rejected write calls `remove(key)` so a failed edit doesn't linger.
//
// Tying to the *edit's own* reload epoch (not a shared counter snapshotted at
// commit) keeps rapid strokes correct: each clears only when its own bake lands.

import { useMemo } from "react";
import { create } from "zustand";

import type { DocumentId } from "@/ipc/pdf";

/** What an overlay needs to redraw a committed-but-not-yet-baked shape. */
export interface HeldEdit {
  /** Unique, stable for this held edit. */
  key: string;
  /** 0-based page index the shape sits on. */
  page: number;
  /** Discriminator the owning layer filters on (`"ink"`, `"text-box"`, …). */
  kind: string;
  /** Per-kind render payload (PDF-space, so it survives zoom/scroll). */
  data: unknown;
  /** Reload epoch that will bake this edit; `null` until the write resolves. */
  epoch: number | null;
}

let seq = 0;
const nextKey = () => `oe-${++seq}`;

interface OptimisticEditState {
  byDoc: Record<DocumentId, HeldEdit[]>;
  /** Highest reload epoch known to have painted, per document. */
  renderedEpoch: Record<DocumentId, number>;
  /** Show a committed shape immediately; returns its key. */
  add: (documentId: DocumentId, page: number, kind: string, data: unknown) => string;
  /** Bind a held edit to the reload epoch that bakes it (after `bumpEpoch`). */
  tie: (documentId: DocumentId, key: string, epoch: number) => void;
  /** Drop a held edit outright (e.g. its write rejected). */
  remove: (documentId: DocumentId, key: string) => void;
  /** A reload for `epoch` has painted: prune everything it baked. */
  markRendered: (documentId: DocumentId, epoch: number) => void;
  /** Forget a document entirely (on close). */
  clearDoc: (documentId: DocumentId) => void;
}

/** True when a held edit is baked on the canvas and can be dropped. */
function baked(held: HeldEdit, renderedEpoch: number): boolean {
  return held.epoch !== null && held.epoch <= renderedEpoch;
}

export const useOptimisticEditStore = create<OptimisticEditState>((set) => ({
  byDoc: {},
  renderedEpoch: {},
  add: (documentId, page, kind, data) => {
    const key = nextKey();
    set((s) => ({
      byDoc: {
        ...s.byDoc,
        [documentId]: [...(s.byDoc[documentId] ?? []), { key, page, kind, data, epoch: null }],
      },
    }));
    return key;
  },
  tie: (documentId, key, epoch) =>
    set((s) => {
      const list = s.byDoc[documentId];
      if (!list) return s;
      const rendered = s.renderedEpoch[documentId] ?? 0;
      // If the baking reload already painted before we could tie (fast edit on a
      // small doc), the edit is on the canvas now — prune instead of keeping it.
      const next =
        epoch <= rendered
          ? list.filter((h) => h.key !== key)
          : list.map((h) => (h.key === key ? { ...h, epoch } : h));
      return { byDoc: { ...s.byDoc, [documentId]: next } };
    }),
  remove: (documentId, key) =>
    set((s) => {
      const list = s.byDoc[documentId];
      if (!list) return s;
      return { byDoc: { ...s.byDoc, [documentId]: list.filter((h) => h.key !== key) } };
    }),
  markRendered: (documentId, epoch) =>
    set((s) => {
      const prevRendered = s.renderedEpoch[documentId] ?? 0;
      const rendered = Math.max(prevRendered, epoch);
      const list = s.byDoc[documentId];
      const nextList = list?.filter((h) => !baked(h, rendered));
      return {
        renderedEpoch: { ...s.renderedEpoch, [documentId]: rendered },
        byDoc: nextList ? { ...s.byDoc, [documentId]: nextList } : s.byDoc,
      };
    }),
  clearDoc: (documentId) =>
    set((s) => {
      const byDoc = { ...s.byDoc };
      const renderedEpoch = { ...s.renderedEpoch };
      delete byDoc[documentId];
      delete renderedEpoch[documentId];
      return { byDoc, renderedEpoch };
    }),
}));

/**
 * Reactive selector: held edits of one `kind` on one page still awaiting their
 * bake. Layers render these with their own draw code (reusing the live-preview
 * path), so no shape logic is duplicated here.
 *
 * Selects the doc's raw list (a reference stable until that doc's held edits
 * change), then derives the page/kind slice in `useMemo` — so we never hand
 * React a fresh array on an unrelated render (which zustand v5 would loop on).
 */
export function usePendingEdits<T>(
  documentId: DocumentId | undefined,
  page: number,
  kind: string,
): Array<{ key: string; data: T }> {
  const list = useOptimisticEditStore((s) => (documentId ? s.byDoc[documentId] : undefined));
  return useMemo(() => {
    if (!list) return [];
    return list
      .filter((h) => h.page === page && h.kind === kind)
      .map((h) => ({ key: h.key, data: h.data as T }));
  }, [list, page, kind]);
}
