// SPEC: P1-VIEW-007 (P1.C4) — full-document text search.
//
// `findRanges` is the pure half — given a string and a query, return
// non-overlapping match ranges. Exported separately so it can be
// unit-tested without any PDF / PDF.js.
//
// `searchDoc` is the side-effecting half — walks every page of an
// already-loaded PDFDocumentProxy, pulls its text content via PDF.js,
// and runs `findRanges` on the concatenation.

import type { PDFDocumentProxy } from "pdfjs-dist";

export interface SearchOptions {
  caseSensitive: boolean;
  wholeWord: boolean;
}

/** Half-open `[start, end)` range of a match within the source text. */
export interface Range {
  start: number;
  end: number;
}

export interface PageMatch {
  pageNumber: number;
  ranges: Range[];
}

/** Escape regex metacharacters so the query is treated as a literal string. */
function escapeForRegex(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

export function findRanges(
  text: string,
  query: string,
  opts: SearchOptions,
): Range[] {
  if (!query || !text) return [];
  const flags = opts.caseSensitive ? "g" : "gi";
  const pattern = opts.wholeWord
    ? `\\b${escapeForRegex(query)}\\b`
    : escapeForRegex(query);
  const re = new RegExp(pattern, flags);

  const out: Range[] = [];
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    // Zero-width match would otherwise loop forever.
    if (m[0].length === 0) {
      re.lastIndex += 1;
      continue;
    }
    out.push({ start: m.index, end: m.index + m[0].length });
  }
  return out;
}

/**
 * Walk every page and return per-page matches. Pages with zero
 * matches are omitted from the result. The implementation is
 * deliberately synchronous-per-page so the caller's "cancelled"
 * flag has frequent checkpoints — see `searchDocStreaming` if you
 * need fine-grained cancellation.
 */
export async function searchDoc(
  doc: PDFDocumentProxy,
  query: string,
  opts: SearchOptions,
  signal?: { cancelled: boolean },
): Promise<PageMatch[]> {
  if (!query) return [];
  const out: PageMatch[] = [];
  for (let i = 1; i <= doc.numPages; i += 1) {
    if (signal?.cancelled) return out;
    const page = await doc.getPage(i);
    const content = await page.getTextContent();
    // PDF.js mixes TextItem (has .str) with TextMarkedContent (no .str)
    // in `content.items`. We only care about the former; flatMap with a
    // narrowing return type filters cleanly without needing the full
    // TextItem type to be assignable.
    const text = content.items
      .flatMap((it) => {
        const s = (it as { str?: unknown }).str;
        return typeof s === "string" ? [s] : [];
      })
      .join(" ");
    const ranges = findRanges(text, query, opts);
    if (ranges.length > 0) out.push({ pageNumber: i, ranges });
  }
  return out;
}

/** Total match count across all pages. */
export function totalMatches(matches: readonly PageMatch[]): number {
  let n = 0;
  for (const m of matches) n += m.ranges.length;
  return n;
}
