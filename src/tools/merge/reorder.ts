// SPEC: P2-PAGE-008 — pure list helpers for the merge dialog's ordered file
// list. Merge order is the whole point, so the reordering logic is kept here,
// unit-tested, and free of UI. All return a NEW array (never mutate input).

/** Move the item at `index` one slot earlier. No-op at the top. */
export function moveUp<T>(items: readonly T[], index: number): T[] {
  if (index <= 0 || index >= items.length) return [...items];
  const next = [...items];
  [next[index - 1], next[index]] = [next[index], next[index - 1]] as [T, T];
  return next;
}

/** Move the item at `index` one slot later. No-op at the bottom. */
export function moveDown<T>(items: readonly T[], index: number): T[] {
  if (index < 0 || index >= items.length - 1) return [...items];
  const next = [...items];
  [next[index], next[index + 1]] = [next[index + 1], next[index]] as [T, T];
  return next;
}

/** Remove the item at `index`. Out-of-range index returns a copy unchanged. */
export function removeAt<T>(items: readonly T[], index: number): T[] {
  if (index < 0 || index >= items.length) return [...items];
  return items.filter((_, i) => i !== index);
}
