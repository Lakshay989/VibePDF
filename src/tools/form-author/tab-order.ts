// SPEC: P5-FORM-006c (P5.B3) — tab-order list mechanics.
//
// The page's tab order is just the order of its field widgets in `/Annots`, so
// reordering is a pure list permutation here; the backend writes it (and sets
// `/Tabs /S`). Kept separate from the panel so the reorder rules are testable
// without React.

/**
 * Move the item at `from` to index `to`, clamping both into range. Returns a new
 * array; the input is untouched. An out-of-range or no-op move returns a copy.
 */
export function moveItem<T>(items: readonly T[], from: number, to: number): T[] {
  const next = [...items];
  if (items.length === 0) return next;
  const src = clamp(from, 0, items.length - 1);
  const dst = clamp(to, 0, items.length - 1);
  if (src === dst) return next;
  const [moved] = next.splice(src, 1);
  // `moved` is defined: `src` is in range and the array is non-empty.
  next.splice(dst, 0, moved as T);
  return next;
}

/** Move the item at `index` one slot earlier (no-op at the top). */
export function moveUp<T>(items: readonly T[], index: number): T[] {
  return moveItem(items, index, index - 1);
}

/** Move the item at `index` one slot later (no-op at the bottom). */
export function moveDown<T>(items: readonly T[], index: number): T[] {
  return moveItem(items, index, index + 1);
}

function clamp(n: number, lo: number, hi: number): number {
  return Math.min(Math.max(n, lo), hi);
}
