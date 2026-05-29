import { create } from "zustand";
import type { DocumentId, OpenedDocument } from "@/ipc/pdf";

interface DocumentState {
  docs: OpenedDocument[];
  currentId: DocumentId | null;
  openDoc: (doc: OpenedDocument) => void;
  closeDoc: (id: DocumentId) => void;
  setCurrent: (id: DocumentId) => void;
  // SPEC: P1-VIEW-011 — bulk restore at startup. Replaces docs +
  // currentId in one set() so the restored session renders once rather
  // than N times. `activePath` selects the active tab; falls back to
  // the first doc when it's missing.
  restoreDocs: (docs: OpenedDocument[], activePath: string | null) => void;
}

export const useDocumentStore = create<DocumentState>((set) => ({
  docs: [],
  currentId: null,
  openDoc: (doc) =>
    set((s) => {
      const existing = s.docs.find((d) => d.path === doc.path);
      if (existing) return { ...s, currentId: existing.id };
      return { docs: [...s.docs, doc], currentId: doc.id };
    }),
  restoreDocs: (docs, activePath) =>
    set(() => {
      if (docs.length === 0) return { docs: [], currentId: null };
      const active = docs.find((d) => d.path === activePath) ?? docs[0];
      return { docs, currentId: active.id };
    }),
  closeDoc: (id) =>
    set((s) => {
      const docs = s.docs.filter((d) => d.id !== id);
      const currentId = s.currentId === id ? (docs[0]?.id ?? null) : s.currentId;
      return { docs, currentId };
    }),
  setCurrent: (id) => set({ currentId: id }),
}));
