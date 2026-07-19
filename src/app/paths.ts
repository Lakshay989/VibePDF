// Small path helpers shared across the app shell (and the viewer, which
// already imports `@/app`). Kept in `src/app/` rather than a new top-level
// dir, per docs/04_ARCHITECTURE.md's "don't add top-level modules lightly"
// rule. `src/tools/` deliberately does not import `@/app`, so the one stamp
// helper that also needs a base name keeps its own inline split.

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
