// SPEC: P1-VIEW-009 (P1.D2).
//
// Normalises PDF.js's outline tree into a serialisable, render-ready
// shape with page numbers already resolved. Kept pure (dest resolution
// is injected via a callback) so it can be unit-tested without a real
// document.
//
// PDF.js's outline:    Array<{ title, dest, items?: [...] }>
// destination shape:   string (named dest) | unknown[] (array form)
// our shape:           { title, page: number | null, children: [...] }

export type RawDestination = string | unknown[];

export interface RawOutlineNode {
  title: string;
  dest: RawDestination | null;
  items?: RawOutlineNode[];
}

export interface NormalizedOutlineNode {
  title: string;
  page: number | null; // null = no destination or unresolvable
  children: NormalizedOutlineNode[];
}

export type DestinationResolver = (
  dest: RawDestination,
) => Promise<number | null>;

/**
 * Convert PDF.js's nested outline into a tree of `NormalizedOutlineNode`.
 * Destinations are resolved to 1-based page numbers via the provided
 * `resolvePage` function, or null when the resolver returns null or
 * the node has no destination at all.
 */
export async function normalizeOutline(
  raw: RawOutlineNode[] | null | undefined,
  resolvePage: DestinationResolver,
): Promise<NormalizedOutlineNode[]> {
  if (!raw || raw.length === 0) return [];
  return Promise.all(
    raw.map(async (node) => {
      const page = node.dest ? await resolvePage(node.dest) : null;
      const children = await normalizeOutline(node.items, resolvePage);
      return { title: node.title, page, children };
    }),
  );
}

/** Count nodes (including children) in a normalized outline. Used to
 *  display the "N entries" badge in the panel header. */
export function countOutlineEntries(
  nodes: readonly NormalizedOutlineNode[],
): number {
  let n = 0;
  for (const node of nodes) {
    n += 1 + countOutlineEntries(node.children);
  }
  return n;
}
