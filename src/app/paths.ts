// Small path helpers shared across the app shell. Kept in `src/app/`
// (next to its only consumers) rather than a new top-level dir, per
// docs/04_ARCHITECTURE.md's "don't add top-level modules lightly" rule.

/**
 * The final path segment of an absolute or relative path. Handles both
 * POSIX (`/`) and Windows (`\`) separators, taking whichever appears
 * rightmost — so a mixed path still resolves correctly. Returns the
 * input unchanged when it contains no separator.
 */
export function basename(path: string): string {
  const sep = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return sep >= 0 ? path.slice(sep + 1) : path;
}
