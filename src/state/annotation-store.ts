// SPEC: infrastructure (P3.A1) — the in-memory annotation store.
//
// IMPORTANT — this is a frontend DRAFT/PREVIEW layer, not a source of truth.
// Like `rotation-preview-store`, it mirrors state the render layer (P3.A2) draws
// and the tools build, but it does NOT own the PDF. Authoritative persistence
// and undo/redo arrive in P3.B1 via the Rust document actor (`pdf/annotation.rs`
// + an `Edit`) — the "actor owns every byte / undo lives in the actor" rule
// (docs/04) still holds. Keeping the two in lockstep is B1's job; A1 only
// provides the staging area so tools and the render layer can be built and
// tested first.

import { create } from "zustand";

import type { DocumentId } from "@/ipc/pdf";
import type { Annotation, AnnotationDraft } from "@/tools/_framework/types";

// Stable empty array so the per-document selector doesn't churn on a miss.
const EMPTY: readonly Annotation[] = Object.freeze([]);

function newId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  return `ann-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`;
}

interface AnnotationState {
  /** Committed annotations, per document. */
  byDoc: Record<DocumentId, Annotation[]>;
  /** The single in-progress draft (the tool currently drawing), or null. */
  draft: AnnotationDraft | null;
  /** The selected annotation's id, or null. */
  selectedId: string | null;

  /** Commit a draft: stamps `id` + `createdAt`, appends it, returns it. */
  add: (id: DocumentId, draft: AnnotationDraft) => Annotation;
  /** Merge a patch into an existing annotation. */
  update: (id: DocumentId, annId: string, patch: Partial<Annotation>) => void;
  /** Remove an annotation; clears the selection if it was selected. */
  remove: (id: DocumentId, annId: string) => void;
  /** Set or clear the live draft. */
  setDraft: (draft: AnnotationDraft | null) => void;
  /** Select an annotation (or clear with null). */
  select: (annId: string | null) => void;
  /** Drop a document's annotations + draft (on close / reload). */
  resetDoc: (id: DocumentId) => void;
}

export const useAnnotationStore = create<AnnotationState>((set) => ({
  byDoc: {},
  draft: null,
  selectedId: null,

  add: (id, draft) => {
    const annotation = { ...draft, id: newId(), createdAt: Date.now() } as Annotation;
    set((s) => ({
      byDoc: { ...s.byDoc, [id]: [...(s.byDoc[id] ?? []), annotation] },
      draft: null,
    }));
    return annotation;
  },

  update: (id, annId, patch) =>
    set((s) => {
      const list = s.byDoc[id];
      if (!list) return s;
      return {
        byDoc: {
          ...s.byDoc,
          [id]: list.map((a) => (a.id === annId ? ({ ...a, ...patch } as Annotation) : a)),
        },
      };
    }),

  remove: (id, annId) =>
    set((s) => {
      const list = s.byDoc[id];
      if (!list) return s;
      return {
        byDoc: { ...s.byDoc, [id]: list.filter((a) => a.id !== annId) },
        selectedId: s.selectedId === annId ? null : s.selectedId,
      };
    }),

  setDraft: (draft) => set({ draft }),

  select: (annId) => set({ selectedId: annId }),

  resetDoc: (id) =>
    set((s) => {
      if (!(id in s.byDoc)) return { draft: null };
      const byDoc = { ...s.byDoc };
      delete byDoc[id];
      return { byDoc, draft: null };
    }),
}));

/** Reactive selector: a document's committed annotations. */
export function useDocAnnotations(id: DocumentId | undefined): readonly Annotation[] {
  return useAnnotationStore((s) => (id ? (s.byDoc[id] ?? EMPTY) : EMPTY));
}
