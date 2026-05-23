import { create } from "zustand";
import type { DocumentId, OpenedDocument } from "@/ipc/pdf";

interface DocumentState {
  docs: OpenedDocument[];
  currentId: DocumentId | null;
  openDoc: (doc: OpenedDocument) => void;
  closeDoc: (id: DocumentId) => void;
  setCurrent: (id: DocumentId) => void;
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
  closeDoc: (id) =>
    set((s) => {
      const docs = s.docs.filter((d) => d.id !== id);
      const currentId = s.currentId === id ? (docs[0]?.id ?? null) : s.currentId;
      return { docs, currentId };
    }),
  setCurrent: (id) => set({ currentId: id }),
}));
