// P6.A1 — the frontend's view of the signature library.
//
// The Rust side is the source of truth for ordering and the cap, so every
// mutation here re-reads the list rather than patching it locally. That costs
// one extra IPC round-trip and buys the guarantee that what the picker shows is
// what is actually on disk — the same reason `commands/recents.rs` returns the
// post-mutation list.
//
// A2–A4 are its consumers: each creates a PNG and calls `add`.

import { create } from "zustand";

import {
  addSignature,
  listSignatures,
  removeSignature,
  type SignatureEntry,
  type SignatureKind,
} from "@/ipc/signatures";

interface SignatureState {
  /** Newest first, mirroring the backend's order. */
  entries: SignatureEntry[];
  /** True while the first load is in flight, so the UI can hold off on "empty". */
  loading: boolean;
  /** Re-read the library from disk. Safe to call repeatedly. */
  refresh: () => Promise<void>;
  /** Store a new signature and return it; the list is refreshed. */
  add: (kind: SignatureKind, png: Uint8Array) => Promise<SignatureEntry>;
  /** Delete one, then refresh. */
  remove: (id: string) => Promise<void>;
}

export const useSignatureStore = create<SignatureState>((set) => ({
  entries: [],
  loading: false,

  refresh: async () => {
    set({ loading: true });
    try {
      set({ entries: await listSignatures() });
    } finally {
      // A failed read leaves the previous list in place rather than blanking
      // the picker; the library is a convenience, not a blocking dependency.
      set({ loading: false });
    }
  },

  add: async (kind, png) => {
    const entry = await addSignature(kind, png);
    set({ entries: await listSignatures() });
    return entry;
  },

  remove: async (id) => {
    await removeSignature(id);
    set({ entries: await listSignatures() });
  },
}));
