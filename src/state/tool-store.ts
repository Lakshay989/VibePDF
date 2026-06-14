// SPEC: infrastructure (P3.A1) — the active annotation tool + its style
// options. Concrete tools subscribe to know if they're active; a toolbar (later)
// drives `setActiveTool`. Named `useToolStore` per docs/04 §State on the
// frontend (previously documented but unbuilt — realized here).

import { create } from "zustand";

import type { ToolId, ToolOptions } from "@/tools/_framework/types";

const DEFAULT_OPTIONS: ToolOptions = {
  // A typical highlighter yellow; tools that don't use colour ignore it.
  color: "#ffd400",
  opacity: 1,
  strokeWidth: 2,
};

interface ToolState {
  /** The active tool, or `null` when none (select/idle). */
  activeTool: ToolId | null;
  options: ToolOptions;
  setActiveTool: (id: ToolId | null) => void;
  setOptions: (patch: Partial<ToolOptions>) => void;
}

export const useToolStore = create<ToolState>((set) => ({
  activeTool: null,
  options: DEFAULT_OPTIONS,
  setActiveTool: (id) => set({ activeTool: id }),
  setOptions: (patch) => set((s) => ({ options: { ...s.options, ...patch } })),
}));
