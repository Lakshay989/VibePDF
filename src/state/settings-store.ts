import { create } from "zustand";
import { getStoredTheme, setStoredTheme, type Theme } from "@/app/theme";

// SPEC: P1-VIEW-012 — last 20 recents, clearable. The list is mirrored
// to Rust on every change (Tauri's app_data_dir) in a follow-up commit;
// for now it lives in-memory + localStorage.
const RECENTS_KEY = "vibepdf.recents";
const RECENTS_MAX = 20;

function loadRecents(): string[] {
  try {
    const raw = localStorage.getItem(RECENTS_KEY);
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter((x): x is string => typeof x === "string").slice(0, RECENTS_MAX);
  } catch {
    return [];
  }
}

function saveRecents(list: string[]): void {
  localStorage.setItem(RECENTS_KEY, JSON.stringify(list));
}

interface SettingsState {
  theme: Theme;
  recents: string[];
  setTheme: (t: Theme) => void;
  pushRecent: (path: string) => void;
  clearRecents: () => void;
}

export const useSettingsStore = create<SettingsState>((set) => ({
  theme: getStoredTheme(),
  recents: loadRecents(),
  setTheme: (t) => {
    setStoredTheme(t);
    set({ theme: t });
  },
  pushRecent: (path) =>
    set((s) => {
      const next = [path, ...s.recents.filter((p) => p !== path)].slice(0, RECENTS_MAX);
      saveRecents(next);
      return { recents: next };
    }),
  clearRecents: () => {
    saveRecents([]);
    set({ recents: [] });
  },
}));
