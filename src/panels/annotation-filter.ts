// SPEC: P3-ANN-008 (P3.D1) — pure filter/group helpers for the annotation
// sidebar. DOM-free so the search + filter logic is unit-testable without React.

import type { AnnotationInfo, AnnotationKind } from "@/ipc/annotations";

/** Human label for a kind (also used as searchable text). */
export const KIND_LABELS: Record<AnnotationKind, string> = {
  highlight: "Highlight",
  underline: "Underline",
  strikeout: "Strikethrough",
  squiggly: "Squiggly",
  note: "Note",
  freetext: "Free text",
  rectangle: "Rectangle",
  ellipse: "Ellipse",
  line: "Line",
  polygon: "Polygon",
  polyline: "Polyline",
  ink: "Ink",
  stamp: "Stamp",
  measure: "Measurement",
};

export function kindLabel(kind: AnnotationKind): string {
  return KIND_LABELS[kind];
}

/** The active filter set. An empty/`null` field means "don't filter on it". */
export interface AnnotationFilter {
  /** Case-insensitive substring matched against contents, author, and kind label. */
  search: string;
  /** Keep only these kinds; empty = all kinds. */
  kinds: AnnotationKind[];
  /** Keep only this exact author; `null` = any author. */
  author: string | null;
  /** Keep only annotations modified at/after this epoch-ms; `null` = any date. */
  modifiedAfter: number | null;
}

export const EMPTY_FILTER: AnnotationFilter = {
  search: "",
  kinds: [],
  author: null,
  modifiedAfter: null,
};

/**
 * Parse a `<input type="date">` value (`YYYY-MM-DD`) as **local** midnight
 * epoch-ms; empty → `null`. A bare `new Date("YYYY-MM-DD")` parses as *UTC*
 * midnight, which in any non-UTC zone shifts the day — so "modified on or after
 * today" would hide annotations made earlier today. Building from parts pins it
 * to the local day the user actually picked.
 */
export function dateInputToMs(value: string): number | null {
  if (!value) return null;
  const [y, m, d] = value.split("-").map(Number);
  if (!y || !m || !d) return null;
  return new Date(y, m - 1, d).getTime();
}

/** Format an epoch-ms as a local `YYYY-MM-DD` for a date input's `value`. */
export function msToDateInput(ms: number | null): string {
  if (ms === null) return "";
  const dt = new Date(ms);
  const y = dt.getFullYear();
  const m = String(dt.getMonth() + 1).padStart(2, "0");
  const d = String(dt.getDate()).padStart(2, "0");
  return `${y}-${m}-${d}`;
}

/** Apply `filter` to `list`, preserving order. */
export function filterAnnotations(
  list: readonly AnnotationInfo[],
  filter: AnnotationFilter,
): AnnotationInfo[] {
  const needle = filter.search.trim().toLowerCase();
  const kindSet = filter.kinds.length > 0 ? new Set(filter.kinds) : null;
  return list.filter((a) => {
    if (kindSet && !kindSet.has(a.kind)) return false;
    if (filter.author !== null && a.author !== filter.author) return false;
    if (filter.modifiedAfter !== null && (a.modified === null || a.modified < filter.modifiedAfter)) {
      return false;
    }
    if (needle) {
      const hay = `${a.contents} ${a.author} ${kindLabel(a.kind)}`.toLowerCase();
      if (!hay.includes(needle)) return false;
    }
    return true;
  });
}

/** A page's annotations, for the grouped list. */
export interface PageGroup {
  /** 0-based page index. */
  page: number;
  items: AnnotationInfo[];
}

/** Group `list` by page, ascending; each group keeps the input order. */
export function groupByPage(list: readonly AnnotationInfo[]): PageGroup[] {
  const byPage = new Map<number, AnnotationInfo[]>();
  for (const a of list) {
    const bucket = byPage.get(a.page);
    if (bucket) bucket.push(a);
    else byPage.set(a.page, [a]);
  }
  return [...byPage.keys()]
    .sort((x, y) => x - y)
    .map((page) => ({ page, items: byPage.get(page) ?? [] }));
}

/** Distinct non-empty authors present in `list`, sorted. */
export function distinctAuthors(list: readonly AnnotationInfo[]): string[] {
  return [...new Set(list.map((a) => a.author).filter((s) => s.length > 0))].sort();
}

/** Distinct kinds present in `list`, in a stable display order. */
export function distinctKinds(list: readonly AnnotationInfo[]): AnnotationKind[] {
  const present = new Set(list.map((a) => a.kind));
  return (Object.keys(KIND_LABELS) as AnnotationKind[]).filter((k) => present.has(k));
}
