// SPEC: P2-PAGE-007 — parse a human "split before these pages" string for
// the split dialog's "at specific pages" mode. Pure + unit-tested so the
// fiddly parsing has no UI dependency.

export type SplitPointsResult = { boundaries: number[] } | { error: string };

/**
 * Parse a comma-separated list of 1-based page numbers like `"4, 7, 10"` into
 * sorted, de-duplicated boundaries where a new file *starts*. Each number is
 * validated against `pageCount`: a split before page 1 (or beyond the last
 * page) is meaningless. Returns `{ boundaries }` or `{ error }`.
 */
export function parseSplitPoints(input: string, pageCount: number): SplitPointsResult {
  const tokens = input
    .split(",")
    .map((t) => t.trim())
    .filter((t) => t.length > 0);
  if (tokens.length === 0) return { error: "Enter at least one split point." };

  const set = new Set<number>();
  for (const tok of tokens) {
    if (!/^\d+$/.test(tok)) return { error: `"${tok}" isn't a page number.` };
    const page = Number(tok);
    if (page <= 1 || page > pageCount) {
      return { error: `Split points must be between 2 and ${pageCount}.` };
    }
    set.add(page);
  }
  return { boundaries: [...set].sort((x, y) => x - y) };
}
