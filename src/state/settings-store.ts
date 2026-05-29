import { create } from "zustand";
import { getStoredTheme, setStoredTheme, type Theme } from "@/app/theme";
import {
  clearRecents as ipcClearRecents,
  listRecents,
  pushRecent as ipcPushRecent,
} from "@/ipc/recents";

// SPEC: P1-VIEW-012 — last 20 recents, clearable. The Rust side owns the
// list (cap-at-20, dedup, persistence to <app_data_dir>/recents.json);
// this store is a mirror hydrated on startup and re-synced from the
// backend's return value on every mutation. We never re-derive order on
// the frontend — whatever Rust returns is authoritative.

interface SettingsState {
  theme: Theme;
  recents: string[];
  setTheme: (t: Theme) => void;
  /** Pull the persisted list from Rust. Call once on app mount. */
  hydrateRecents: () => Promise<void>;
  /** Record a freshly-opened path; mirrors the backend's new list. */
  pushRecent: (path: string) => Promise<void>;
  /** Clear on UI and disk. */
  clearRecents: () => Promise<void>;
}

export const useSettingsStore = create<SettingsState>((set) => ({
  theme: getStoredTheme(),
  recents: [],
  setTheme: (t) => {
    setStoredTheme(t);
    set({ theme: t });
  },
  hydrateRecents: async () => {
    try {
      set({ recents: await listRecents() });
    } catch (err) {
      // Recents are a convenience; a read failure must not block the
      // start screen. Leave the list empty and log.
      console.warn("hydrateRecents failed", err);
    }
  },
  pushRecent: async (path) => {
    try {
      set({ recents: await ipcPushRecent(path) });
    } catch (err) {
      console.warn("pushRecent failed", path, err);
    }
  },
  clearRecents: async () => {
    try {
      set({ recents: await ipcClearRecents() });
    } catch (err) {
      console.warn("clearRecents failed", err);
    }
  },
}));
