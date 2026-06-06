// SPEC: P2-PAGE-006 — parse a human page-range string for the extract
// dialog. Pure + unit-tested so the fiddly parsing has no UI dependency.

export type PageRangeResult = { pages: number[] } | { error: string };

/**
 * Parse a 1-based page-range string like `"2-3, 5, 8-10"` into sorted,
 * de-duplicated **0-based** page indices, validated against `pageCount`.
 * Returns `{ pages }` on success or `{ error }` with a human message.
 */
export function parsePageRange(input: string, pageCount: number): PageRangeResult {
  const tokens = input
    .split(",")
    .map((t) => t.trim())
    .filter((t) => t.length > 0);
  if (tokens.length === 0) return { error: "Enter at least one page." };

  const set = new Set<number>();
  for (const tok of tokens) {
    const m = /^(\d+)(?:\s*-\s*(\d+))?$/.exec(tok);
    if (!m) return { error: `"${tok}" isn't a page or range.` };
    const a = Number(m[1]);
    const b = m[2] === undefined ? a : Number(m[2]);
    const lo = Math.min(a, b);
    const hi = Math.max(a, b);
    if (lo < 1 || hi > pageCount) {
      return { error: `Pages must be between 1 and ${pageCount}.` };
    }
    for (let p = lo; p <= hi; p += 1) set.add(p - 1); // 1-based → 0-based
  }
  return { pages: [...set].sort((x, y) => x - y) };
}
