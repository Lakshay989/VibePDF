import { useEffect, useState } from "react";
import type { PDFDocumentProxy } from "pdfjs-dist";

import {
  countOutlineEntries,
  normalizeOutline,
  type NormalizedOutlineNode,
  type RawDestination,
} from "@/panels/outline-tree";

// SPEC: P1-VIEW-009 (P1.D2).
//
// The panel loads the document's outline once, resolves every
// destination to a page number via PDF.js, and renders a collapsible
// tree. Clicking an entry calls `onJump(pageNumber)`.

interface Props {
  doc: PDFDocumentProxy;
  onJump: (page: number) => void;
}

export function OutlinePanel({ doc, onJump }: Props) {
  const [tree, setTree] = useState<NormalizedOutlineNode[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setTree(null);
    setError(null);
    (async () => {
      try {
        const raw = (await doc.getOutline()) as
          | Array<{ title: string; dest: RawDestination | null; items?: unknown[] }>
          | null;
        const resolvePage = async (
          dest: RawDestination,
        ): Promise<number | null> => {
          try {
            let arr: unknown[];
            if (typeof dest === "string") {
              const resolved = await doc.getDestination(dest);
              if (!resolved) return null;
              arr = resolved;
            } else {
              arr = dest;
            }
            const ref = arr[0];
            // PDF.js types this as RefProxy; getPageIndex accepts it.
            const idx = await doc.getPageIndex(ref as never);
            return idx + 1;
          } catch {
            return null;
          }
        };
        const normalized = await normalizeOutline(
          (raw ?? []) as unknown as Parameters<typeof normalizeOutline>[0],
          resolvePage,
        );
        if (!cancelled) setTree(normalized);
      } catch (e) {
        if (!cancelled) {
          setError(
            e instanceof Error ? e.message : "Failed to read outline.",
          );
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [doc]);

  return (
    <aside className="flex h-full w-60 shrink-0 flex-col border-r border-neutral-200 bg-neutral-50 dark:border-neutral-800 dark:bg-neutral-950">
      <header className="border-b border-neutral-200 px-3 py-2 text-xs font-semibold uppercase tracking-wide text-neutral-500 dark:border-neutral-800">
        Outline
        {tree && tree.length > 0 ? (
          <span className="ml-1 text-neutral-400">
            ({countOutlineEntries(tree)})
          </span>
        ) : null}
      </header>
      <div className="flex-1 overflow-auto p-2 text-sm">
        {error ? (
          <div className="text-red-600 dark:text-red-400">{error}</div>
        ) : tree === null ? (
          <div className="text-neutral-500">Loading…</div>
        ) : tree.length === 0 ? (
          <div className="text-neutral-500">No outline.</div>
        ) : (
          <ul className="space-y-0.5">
            {tree.map((node, i) => (
              <OutlineEntry key={i} node={node} depth={0} onJump={onJump} />
            ))}
          </ul>
        )}
      </div>
    </aside>
  );
}

interface EntryProps {
  node: NormalizedOutlineNode;
  depth: number;
  onJump: (page: number) => void;
}

function OutlineEntry({ node, depth, onJump }: EntryProps) {
  const [open, setOpen] = useState(true);
  const hasChildren = node.children.length > 0;

  return (
    <li>
      <div
        className="flex items-start gap-1 rounded px-1 py-0.5 hover:bg-neutral-100 dark:hover:bg-neutral-900"
        style={{ paddingLeft: 4 + depth * 12 }}
      >
        {hasChildren ? (
          <button
            type="button"
            onClick={() => setOpen((o) => !o)}
            aria-label={open ? "Collapse" : "Expand"}
            className="flex h-5 w-4 shrink-0 items-center justify-center text-neutral-400 hover:text-neutral-700 dark:hover:text-neutral-200"
          >
            {open ? "▾" : "▸"}
          </button>
        ) : (
          <span className="w-4 shrink-0" />
        )}
        <button
          type="button"
          onClick={() => node.page != null && onJump(node.page)}
          disabled={node.page == null}
          title={node.page != null ? `Page ${node.page}` : "No destination"}
          className="flex-1 truncate text-left disabled:cursor-not-allowed disabled:text-neutral-400"
        >
          {node.title}
        </button>
        {node.page != null ? (
          <span className="shrink-0 text-xs tabular-nums text-neutral-400">
            {node.page}
          </span>
        ) : null}
      </div>
      {hasChildren && open ? (
        <ul className="space-y-0.5">
          {node.children.map((child, i) => (
            <OutlineEntry key={i} node={child} depth={depth + 1} onJump={onJump} />
          ))}
        </ul>
      ) : null}
    </li>
  );
}
