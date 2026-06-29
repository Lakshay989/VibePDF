// SPEC: P4-EDIT-009 / P4-EDIT-008 — shared page-range parsing for the
// document-wide page-decoration dialogs (watermark, background, …). Pure, so it
// has no UI / IPC dependency and is unit-tested directly.

export type PageRangeResult = { pages: number[] } | { error: string };

/**
 * Parse a page-range spec into sorted, de-duplicated **0-based** indices.
 * `"all"` (or empty) → every page. Otherwise a comma list of 1-based numbers and
 * `a-b` ranges, e.g. `"1-3, 5, 8-10"`. Out-of-range or malformed → `{ error }`.
 */
export function parsePageRange(input: string, pageCount: number): PageRangeResult {
  const trimmed = input.trim().toLowerCase();
  if (trimmed === "" || trimmed === "all") {
    return { pages: Array.from({ length: pageCount }, (_, i) => i) };
  }

  const set = new Set<number>();
  for (const raw of trimmed.split(",")) {
    const tok = raw.trim();
    if (tok === "") continue;
    const range = /^(\d+)\s*-\s*(\d+)$/.exec(tok);
    const single = /^(\d+)$/.exec(tok);
    if (range) {
      const lo = Number(range[1]);
      const hi = Number(range[2]);
      if (lo < 1 || hi > pageCount || lo > hi) {
        return { error: `Range "${tok}" is outside 1–${pageCount}.` };
      }
      for (let p = lo; p <= hi; p++) set.add(p - 1);
    } else if (single) {
      const p = Number(single[1]);
      if (p < 1 || p > pageCount) {
        return { error: `Page ${p} is outside 1–${pageCount}.` };
      }
      set.add(p - 1);
    } else {
      return { error: `"${tok}" isn't a page or range.` };
    }
  }
  if (set.size === 0) return { error: "Enter at least one page." };
  return { pages: [...set].sort((a, b) => a - b) };
}
