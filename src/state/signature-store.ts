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
  signatureBytes,
  type SignatureEntry,
  type SignatureKind,
} from "@/ipc/signatures";
import { bytesToDataUrl } from "@/view/file-data-url";

interface SignatureState {
  /** Newest first, mirroring the backend's order. */
  entries: SignatureEntry[];
  /** True while the first load is in flight, so the UI can hold off on "empty". */
  loading: boolean;
  /**
   * P6.A5a — `data:` URLs by entry id, for the picker's thumbnails.
   *
   * A list of kinds and dates is not something anyone can choose a signature
   * from; you have to see them. Cached because the bytes cross IPC as a JSON
   * number array at roughly 4× their size, and the library holds up to 20.
   */
  thumbs: Record<string, string>;
  /** Re-read the library from disk. Safe to call repeatedly. */
  refresh: () => Promise<void>;
  /** Store a new signature and return it; the list is refreshed. */
  add: (kind: SignatureKind, png: Uint8Array) => Promise<SignatureEntry>;
  /** Delete one, then refresh. */
  remove: (id: string) => Promise<void>;
  /** Fetch and cache one thumbnail. A second call for the same id is free. */
  loadThumb: (id: string) => Promise<void>;
}

export const useSignatureStore = create<SignatureState>((set, get) => ({
  entries: [],
  loading: false,
  thumbs: {},

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
    // Drop the thumbnail too: ids are not reused, so keeping it would only be a
    // leak that grows with every delete.
    const thumbs = { ...get().thumbs };
    delete thumbs[id];
    set({ entries: await listSignatures(), thumbs });
  },

  loadThumb: async (id) => {
    if (get().thumbs[id]) return;
    const url = bytesToDataUrl(await signatureBytes(id));
    // Re-read rather than closing over the old map — several thumbnails load
    // concurrently, and a stale spread would drop all but the last.
    set((s) => ({ thumbs: { ...s.thumbs, [id]: url } }));
  },
}));
