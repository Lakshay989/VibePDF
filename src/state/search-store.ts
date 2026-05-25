import { create } from "zustand";

import type { PageMatch } from "@/view/search";

// SPEC: P1-VIEW-007 (P1.C4).
//
// The store is intentionally dumb: it holds the query, the options,
// the result of the latest search, and the position-in-list. It does
// NOT run the search — that's a side effect owned by PdfViewer, which
// kicks off searchDoc whenever query/options change and writes the
// result back via `setMatches`.

interface FlatMatch {
  pageNumber: number;
  /** Index *into the per-page ranges* of this match. */
  rangeIndex: number;
}

function flatten(matches: readonly PageMatch[]): FlatMatch[] {
  const out: FlatMatch[] = [];
  for (const m of matches) {
    for (let i = 0; i < m.ranges.length; i += 1) {
      out.push({ pageNumber: m.pageNumber, rangeIndex: i });
    }
  }
  return out;
}

interface SearchState {
  isOpen: boolean;
  query: string;
  caseSensitive: boolean;
  wholeWord: boolean;
  matches: PageMatch[];
  flat: FlatMatch[];
  currentIndex: number; // 0-based into `flat`; -1 when no matches.
  searching: boolean;

  open: () => void;
  close: () => void;
  setQuery: (q: string) => void;
  toggleCaseSensitive: () => void;
  toggleWholeWord: () => void;
  setSearching: (b: boolean) => void;
  setMatches: (m: PageMatch[]) => void;
  next: () => void;
  prev: () => void;
  reset: () => void;
}

export const useSearchStore = create<SearchState>((set) => ({
  isOpen: false,
  query: "",
  caseSensitive: false,
  wholeWord: false,
  matches: [],
  flat: [],
  currentIndex: -1,
  searching: false,

  open: () => set({ isOpen: true }),
  close: () => set({ isOpen: false }),
  setQuery: (q) => set({ query: q }),
  toggleCaseSensitive: () =>
    set((s) => ({ caseSensitive: !s.caseSensitive })),
  toggleWholeWord: () => set((s) => ({ wholeWord: !s.wholeWord })),
  setSearching: (b) => set({ searching: b }),
  setMatches: (m) => {
    const flat = flatten(m);
    set({ matches: m, flat, currentIndex: flat.length > 0 ? 0 : -1 });
  },
  next: () =>
    set((s) => {
      if (s.flat.length === 0) return s;
      return { currentIndex: (s.currentIndex + 1) % s.flat.length };
    }),
  prev: () =>
    set((s) => {
      if (s.flat.length === 0) return s;
      return {
        currentIndex:
          (s.currentIndex - 1 + s.flat.length) % s.flat.length,
      };
    }),
  reset: () =>
    set({
      query: "",
      matches: [],
      flat: [],
      currentIndex: -1,
      searching: false,
    }),
}));
