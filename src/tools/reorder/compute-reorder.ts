// SPEC: P2-PAGE-002 — pure helper for thumbnail drag-reorder. Turns a single
// "move the page at `from` to position `to`" drag into the full permutation the
// backend expects (`result[newPos] = oldIndex`). Kept here, unit-tested, free
// of UI so the off-by-one-prone bit has no DOM dependency.

/**
 * The new page order after moving the page currently at index `from` to index
 * `to` in a `count`-page document. Returns `[0, 1, …, count-1]` with `from`
 * spliced out and reinserted at `to`. A no-op move (`from === to`, or
 * out-of-range) returns the identity order.
 */
export function movePage(count: number, from: number, to: number): number[] {
  const identity = Array.from({ length: count }, (_, i) => i);
  if (
    from === to ||
    from < 0 ||
    to < 0 ||
    from >= count ||
    to >= count
  ) {
    return identity;
  }
  const order = identity.slice();
  const [moved] = order.splice(from, 1);
  order.splice(to, 0, moved as number);
  return order;
}

/** True when `order` is a permutation of `0..order.length` (defensive guard). */
export function isPermutation(order: number[]): boolean {
  const seen = new Array<boolean>(order.length).fill(false);
  for (const i of order) {
    if (!Number.isInteger(i) || i < 0 || i >= order.length || seen[i]) return false;
    seen[i] = true;
  }
  return true;
}
