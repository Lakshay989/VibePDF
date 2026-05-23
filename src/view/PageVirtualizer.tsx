import { useEffect, useMemo, useRef, useState } from "react";
import type { PDFDocumentProxy } from "pdfjs-dist";

import { renderPageOnDoc } from "@/view/render-page";
import { LruCache } from "@/view/page-cache";

// SPEC: P1-VIEW-005, NFR-PERF-003.
//
// Strategy:
//  1. On mount, fetch every page's viewport (cheap; PDF.js does not
//     rasterize) so we know each slot's exact height up-front. That
//     lets the scrollbar sit at the right absolute position from the
//     first frame, before any page has rendered.
//  2. Each page gets a fixed-height <div> slot in a tall stack. The
//     virtualizer only mounts a <canvas> into a slot when its
//     IntersectionObserver entry crosses the +/- 200 % rootMargin
//     band (≈ two viewport heights either side). Pages outside the
//     band are placeholders → no canvas in the DOM, no GPU memory.
//  3. Recently-rendered canvases are kept in an LRU keyed by
//     `${page}:${scale}:${dpr}` so rapid scroll-back is instant.
//     Capacity 50 = roughly ten viewports of warm cache at A4 zoom.

const PRE_RENDER_MARGIN = "200% 0% 200% 0%";
const CACHE_CAPACITY = 50;

interface PageInfo {
  pageNumber: number;
  cssWidth: number;
  cssHeight: number;
}

interface Props {
  doc: PDFDocumentProxy;
  documentId: string;
  scale?: number;
}

export function PageVirtualizer({ doc, documentId, scale = 1.5 }: Props) {
  const [pages, setPages] = useState<PageInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  // One LRU per virtualizer instance. Lifecycle tied to the doc.
  const cacheRef = useRef<LruCache<HTMLCanvasElement>>(
    new LruCache<HTMLCanvasElement>(CACHE_CAPACITY),
  );

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const out: PageInfo[] = [];
        for (let i = 1; i <= doc.numPages; i += 1) {
          const page = await doc.getPage(i);
          const v = page.getViewport({ scale });
          out.push({
            pageNumber: i,
            cssWidth: Math.floor(v.width),
            cssHeight: Math.floor(v.height),
          });
        }
        if (!cancelled) setPages(out);
      } catch (e) {
        if (!cancelled) {
          setError(
            e instanceof Error ? e.message : "Could not read page dimensions.",
          );
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [doc, scale]);

  // Reset the cache when the document or zoom changes.
  useEffect(() => {
    cacheRef.current.clear();
  }, [documentId, scale]);

  if (error) {
    return (
      <div className="mx-auto max-w-lg p-4 text-sm text-red-700 dark:text-red-300">
        {error}
      </div>
    );
  }
  if (!pages) {
    return (
      <div className="p-4 text-sm text-neutral-500">Reading pages…</div>
    );
  }

  return (
    <div className="flex flex-col items-center gap-4 py-4">
      {pages.map((info) => (
        <PageSlot
          key={info.pageNumber}
          info={info}
          doc={doc}
          scale={scale}
          documentId={documentId}
          cache={cacheRef.current}
        />
      ))}
    </div>
  );
}

interface SlotProps {
  info: PageInfo;
  doc: PDFDocumentProxy;
  scale: number;
  documentId: string;
  cache: LruCache<HTMLCanvasElement>;
}

function PageSlot({ info, doc, scale, documentId, cache }: SlotProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [visible, setVisible] = useState(false);

  const cacheKey = useMemo(() => {
    const dpr =
      typeof window !== "undefined" ? window.devicePixelRatio || 1 : 1;
    return `${documentId}:${info.pageNumber}:${scale}:${dpr}`;
  }, [documentId, info.pageNumber, scale]);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          setVisible(entry.isIntersecting);
        }
      },
      { rootMargin: PRE_RENDER_MARGIN },
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  // When the slot becomes visible, render (or pull from cache) into
  // a canvas appended to the slot. When it leaves the band, drop the
  // canvas from the DOM but keep the bitmap in the LRU.
  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    if (!visible) {
      while (el.firstChild) el.removeChild(el.firstChild);
      return;
    }

    const cached = cache.get(cacheKey);
    if (cached) {
      el.appendChild(cached);
      return;
    }

    let cancelled = false;
    const canvas = document.createElement("canvas");
    canvas.style.maxWidth = "100%";
    canvas.className = "shadow";
    el.appendChild(canvas);

    void renderPageOnDoc({
      doc,
      pageNumber: info.pageNumber,
      scale,
      canvas,
    })
      .then(() => {
        if (!cancelled) cache.set(cacheKey, canvas);
      })
      .catch((err) => {
        console.warn(
          `page ${info.pageNumber} render failed:`,
          err instanceof Error ? err.message : err,
        );
      });

    return () => {
      cancelled = true;
    };
  }, [visible, cacheKey, cache, doc, info.pageNumber, scale]);

  return (
    <div
      ref={containerRef}
      data-page={info.pageNumber}
      style={{ width: info.cssWidth, height: info.cssHeight }}
      className="relative bg-white dark:bg-neutral-100"
    />
  );
}
