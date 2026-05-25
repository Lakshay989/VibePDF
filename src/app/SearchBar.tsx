import { useEffect, useRef } from "react";

import { useSearchStore } from "@/state/search-store";

// SPEC: P1-VIEW-007 (P1.C4).
//
// Thin presentational component. All state and behavior live in
// search-store; PdfViewer orchestrates open/close on Cmd+F / Escape
// and runs the actual search effect.

export function SearchBar() {
  const isOpen = useSearchStore((s) => s.isOpen);
  const query = useSearchStore((s) => s.query);
  const caseSensitive = useSearchStore((s) => s.caseSensitive);
  const wholeWord = useSearchStore((s) => s.wholeWord);
  const flat = useSearchStore((s) => s.flat);
  const currentIndex = useSearchStore((s) => s.currentIndex);
  const searching = useSearchStore((s) => s.searching);
  const setQuery = useSearchStore((s) => s.setQuery);
  const toggleCaseSensitive = useSearchStore((s) => s.toggleCaseSensitive);
  const toggleWholeWord = useSearchStore((s) => s.toggleWholeWord);
  const next = useSearchStore((s) => s.next);
  const prev = useSearchStore((s) => s.prev);
  const close = useSearchStore((s) => s.close);

  const inputRef = useRef<HTMLInputElement>(null);

  // Auto-focus + select-all when the bar opens, so consecutive Cmd+F
  // presses without typing reuse the existing query.
  useEffect(() => {
    if (!isOpen) return;
    const el = inputRef.current;
    if (!el) return;
    el.focus();
    el.select();
  }, [isOpen]);

  if (!isOpen) return null;

  const hasMatches = flat.length > 0;
  const status = (() => {
    if (searching) return "Searching…";
    if (!query) return " ";
    if (!hasMatches) return "No matches";
    return `${currentIndex + 1} of ${flat.length}`;
  })();

  return (
    <div className="flex items-center gap-2 border-b border-neutral-200 bg-neutral-50 px-3 py-1.5 text-sm dark:border-neutral-800 dark:bg-neutral-950">
      <input
        ref={inputRef}
        value={query}
        onChange={(e) => setQuery(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            if (e.shiftKey) prev();
            else next();
          } else if (e.key === "Escape") {
            e.preventDefault();
            close();
          }
        }}
        placeholder="Search…"
        className="w-64 rounded border border-neutral-300 bg-transparent px-2 py-0.5 dark:border-neutral-700"
        aria-label="Search query"
      />
      <button
        type="button"
        onClick={toggleCaseSensitive}
        aria-pressed={caseSensitive}
        title="Case sensitive"
        className={
          "rounded px-2 py-0.5 font-mono text-xs hover:bg-neutral-100 dark:hover:bg-neutral-800 " +
          (caseSensitive ? "bg-neutral-200 dark:bg-neutral-800" : "")
        }
      >
        Aa
      </button>
      <button
        type="button"
        onClick={toggleWholeWord}
        aria-pressed={wholeWord}
        title="Whole word"
        className={
          "rounded px-2 py-0.5 font-mono text-xs hover:bg-neutral-100 dark:hover:bg-neutral-800 " +
          (wholeWord ? "bg-neutral-200 dark:bg-neutral-800" : "")
        }
      >
        \b
      </button>
      <div className="min-w-[7ch] text-xs tabular-nums text-neutral-500">
        {status}
      </div>
      <button
        type="button"
        onClick={prev}
        disabled={!hasMatches}
        aria-label="Previous match"
        title="Previous (Shift+Enter)"
        className="rounded px-2 py-0.5 hover:bg-neutral-100 disabled:cursor-not-allowed disabled:opacity-40 dark:hover:bg-neutral-800"
      >
        ↑
      </button>
      <button
        type="button"
        onClick={next}
        disabled={!hasMatches}
        aria-label="Next match"
        title="Next (Enter)"
        className="rounded px-2 py-0.5 hover:bg-neutral-100 disabled:cursor-not-allowed disabled:opacity-40 dark:hover:bg-neutral-800"
      >
        ↓
      </button>
      <button
        type="button"
        onClick={close}
        aria-label="Close search"
        title="Close (Esc)"
        className="ml-auto rounded px-2 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-800"
      >
        ✕
      </button>
    </div>
  );
}
