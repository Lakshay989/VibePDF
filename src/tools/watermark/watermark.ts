// SPEC: P4-EDIT-009 (P4.D2) — pure helpers for the watermark dialog: the spec
// shape, the page-range parser, and a default. No PDF / IPC, so it's unit-tested
// directly.

/** A watermark's appearance + placement, independent of text-vs-image source. */
export interface WatermarkCommon {
  /** 0-based page indices to stamp. */
  pages: number[];
  /** 0..1. */
  opacity: number;
  /** Degrees, counter-clockwise. */
  rotation: number;
  /** Draw under existing content (vs. on top). */
  behind: boolean;
}

export type WatermarkSpec =
  | (WatermarkCommon & {
      kind: "text";
      text: string;
      fontFamily: string;
      fontSize: number;
      color: string;
    })
  | (WatermarkCommon & { kind: "image"; imagePath: string });

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

/** Sensible defaults for a fresh watermark: a faint grey "DRAFT" at 45°. */
export const DEFAULT_WATERMARK = {
  text: "DRAFT",
  fontFamily: "Helvetica",
  fontSize: 72,
  color: "#808080",
  opacity: 0.3,
  rotation: 45,
  behind: true,
} as const;
