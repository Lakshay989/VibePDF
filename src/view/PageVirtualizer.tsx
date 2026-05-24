import {
  forwardRef,
  useEffect,
  useImperativeHandle,
  useMemo,
  useRef,
  useState,
} from "react";
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
//
// The component owns its own scroll container (it's the
// `overflow-auto` root) and exposes an imperative API via ref so
// PdfViewer can wire keyboard nav into it without prop-drilling.

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

export interface PageVirtualizerHandle {
  scrollToPage: (page: number) => void;
  scrollByPages: (delta: number) => void;
  scrollByLine: (deltaPx: number) => void;
  getCurrentPage: () => number;
}

export const PageVirtualizer = forwardRef<PageVirtualizerHandle, Props>(
  function PageVirtualizer({ doc, documentId, scale = 1.5 }, ref) {
    const [pages, setPages] = useState<PageInfo[] | null>(null);
    const [error, setError] = useState<string | null>(null);

    const cacheRef = useRef<LruCache<HTMLCanvasElement>>(
      new LruCache<HTMLCanvasElement>(CACHE_CAPACITY),
    );
    const scrollRef = useRef<HTMLDivElement>(null);
    const slotsRef = useRef<Map<number, HTMLDivElement>>(new Map());
    const currentPageRef = useRef(1);

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
              e instanceof Error
                ? e.message
                : "Could not read page dimensions.",
            );
          }
        }
      })();
      return () => {
        cancelled = true;
      };
    }, [doc, scale]);

    useEffect(() => {
      cacheRef.current.clear();
      slotsRef.current.clear();
      currentPageRef.current = 1;
    }, [documentId, scale]);

    // Track which page is currently at (or just below) the viewport top.
    // Used by PageDown/PageUp navigation. Recomputed on scroll.
    useEffect(() => {
      const scroller = scrollRef.current;
      if (!scroller || !pages) return;
      const onScroll = () => {
        const top = scroller.scrollTop;
        let best = 1;
        for (const info of pages) {
          const el = slotsRef.current.get(info.pageNumber);
          if (!el) continue;
          if (el.offsetTop <= top + 8) best = info.pageNumber;
          else break;
        }
        currentPageRef.current = best;
      };
      scroller.addEventListener("scroll", onScroll, { passive: true });
      onScroll();
      return () => scroller.removeEventListener("scroll", onScroll);
    }, [pages]);

    useImperativeHandle(
      ref,
      () => ({
        scrollToPage: (page: number) => {
          if (!pages) return;
          const clamped = Math.max(1, Math.min(pages.length, page));
          const el = slotsRef.current.get(clamped);
          const scroller = scrollRef.current;
          if (!el || !scroller) return;
          scroller.scrollTo({
            top: el.offsetTop - 8,
            behavior: "smooth",
          });
          currentPageRef.current = clamped;
        },
        scrollByPages: (delta: number) => {
          if (!pages) return;
          const target = currentPageRef.current + delta;
          const clamped = Math.max(1, Math.min(pages.length, target));
          const el = slotsRef.current.get(clamped);
          const scroller = scrollRef.current;
          if (!el || !scroller) return;
          scroller.scrollTo({
            top: el.offsetTop - 8,
            behavior: "smooth",
          });
          currentPageRef.current = clamped;
        },
        scrollByLine: (deltaPx: number) => {
          scrollRef.current?.scrollBy({ top: deltaPx, behavior: "auto" });
        },
        getCurrentPage: () => currentPageRef.current,
      }),
      [pages],
    );

    const registerSlot = (page: number, el: HTMLDivElement | null) => {
      if (el) slotsRef.current.set(page, el);
      else slotsRef.current.delete(page);
    };

    return (
      <div
        ref={scrollRef}
        className="h-full overflow-auto bg-neutral-100 dark:bg-neutral-900"
        tabIndex={0}
      >
        {error ? (
          <div className="mx-auto mt-8 max-w-lg p-4 text-sm text-red-700 dark:text-red-300">
            {error}
          </div>
        ) : !pages ? (
          <div className="p-4 text-sm text-neutral-500">Reading pages…</div>
        ) : (
          <div className="flex flex-col items-center gap-4 py-4">
            {pages.map((info) => (
              <PageSlot
                key={info.pageNumber}
                info={info}
                doc={doc}
                scale={scale}
                documentId={documentId}
                cache={cacheRef.current}
                onMount={(el) => registerSlot(info.pageNumber, el)}
              />
            ))}
          </div>
        )}
      </div>
    );
  },
);

interface SlotProps {
  info: PageInfo;
  doc: PDFDocumentProxy;
  scale: number;
  documentId: string;
  cache: LruCache<HTMLCanvasElement>;
  onMount: (el: HTMLDivElement | null) => void;
}

function PageSlot({
  info,
  doc,
  scale,
  documentId,
  cache,
  onMount,
}: SlotProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [visible, setVisible] = useState(false);

  const cacheKey = useMemo(() => {
    const dpr =
      typeof window !== "undefined" ? window.devicePixelRatio || 1 : 1;
    return `${documentId}:${info.pageNumber}:${scale}:${dpr}`;
  }, [documentId, info.pageNumber, scale]);

  useEffect(() => {
    onMount(containerRef.current);
    return () => onMount(null);
  }, [onMount]);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) setVisible(entry.isIntersecting);
      },
      { rootMargin: PRE_RENDER_MARGIN },
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

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
