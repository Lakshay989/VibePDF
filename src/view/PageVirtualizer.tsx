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
import { invertCanvasForDarkMode } from "@/view/dark-invert";
import type { FitMode } from "@/state/view-store";

// SPEC: P1-VIEW-005, P1-VIEW-006, NFR-PERF-003.
//
// Strategy:
//  1. Fetch each page's NATURAL (scale = 1) dimensions on mount. The
//     effective scale is computed from `zoom` (when fitMode is null)
//     or from `fitMode` + the live container size (otherwise).
//  2. A ResizeObserver on the scroll container recomputes the
//     effective scale on resize when fitMode is active. No-op when
//     fitMode is null.
//  3. Each page gets a fixed-height <div> sized at natural × scale.
//     Canvases are mounted only when the slot enters the +/-200%
//     IntersectionObserver band.
//  4. Recently-rendered canvases are kept in an LRU (capacity 50)
//     keyed on `${doc}:${page}:${scale}:${dpr}` so rapid scroll-back
//     is instant.

const PRE_RENDER_MARGIN = "200% 0% 200% 0%";
const CACHE_CAPACITY = 50;
const PAGE_GAP_PX = 16;

interface NaturalPage {
  pageNumber: number;
  width: number;
  height: number;
}

interface Props {
  doc: PDFDocumentProxy;
  documentId: string;
  zoom: number;
  fitMode: FitMode | null;
  darkMode: boolean;
}

export interface PageVirtualizerHandle {
  scrollToPage: (page: number) => void;
  scrollByPages: (delta: number) => void;
  scrollByLine: (deltaPx: number) => void;
  getCurrentPage: () => number;
}

function computeFitScale(
  mode: FitMode,
  containerW: number,
  containerH: number,
  pageW: number,
  pageH: number,
): number {
  // Subtract some chrome so the page doesn't kiss the scrollbar.
  const w = Math.max(1, containerW - PAGE_GAP_PX * 2);
  const h = Math.max(1, containerH - PAGE_GAP_PX * 2);
  switch (mode) {
    case "actual":
      return 1;
    case "fit-width":
      return w / pageW;
    case "fit-height":
      return h / pageH;
    case "fit-page":
      return Math.min(w / pageW, h / pageH);
  }
}

export const PageVirtualizer = forwardRef<PageVirtualizerHandle, Props>(
  function PageVirtualizer(
    { doc, documentId, zoom, fitMode, darkMode },
    ref,
  ) {
    const [pages, setPages] = useState<NaturalPage[] | null>(null);
    const [error, setError] = useState<string | null>(null);
    const [containerSize, setContainerSize] = useState<{
      w: number;
      h: number;
    } | null>(null);

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
          const out: NaturalPage[] = [];
          for (let i = 1; i <= doc.numPages; i += 1) {
            const page = await doc.getPage(i);
            const v = page.getViewport({ scale: 1 });
            out.push({
              pageNumber: i,
              width: v.width,
              height: v.height,
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
    }, [doc]);

    useEffect(() => {
      cacheRef.current.clear();
      slotsRef.current.clear();
      currentPageRef.current = 1;
    }, [documentId]);

    // Observe container size for fit-mode recomputation.
    useEffect(() => {
      const el = scrollRef.current;
      if (!el) return;
      const update = () =>
        setContainerSize({ w: el.clientWidth, h: el.clientHeight });
      update();
      const ro = new ResizeObserver(update);
      ro.observe(el);
      return () => ro.disconnect();
    }, []);

    const effectiveScale = useMemo(() => {
      if (!pages || pages.length === 0) return zoom;
      if (!fitMode) return zoom;
      if (!containerSize) return zoom;
      const first = pages[0];
      return computeFitScale(
        fitMode,
        containerSize.w,
        containerSize.h,
        first.width,
        first.height,
      );
    }, [pages, zoom, fitMode, containerSize]);

    // Drop bitmap cache whenever the effective scale or theme
    // changes. Both are in the per-slot cache key, so stale entries
    // would never be re-used; clearing also frees memory promptly.
    useEffect(() => {
      cacheRef.current.clear();
    }, [effectiveScale, darkMode]);

    // Track which page is currently at (or just below) the viewport top.
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
          scroller.scrollTo({ top: el.offsetTop - 8, behavior: "smooth" });
          currentPageRef.current = clamped;
        },
        scrollByPages: (delta: number) => {
          if (!pages) return;
          const target = currentPageRef.current + delta;
          const clamped = Math.max(1, Math.min(pages.length, target));
          const el = slotsRef.current.get(clamped);
          const scroller = scrollRef.current;
          if (!el || !scroller) return;
          scroller.scrollTo({ top: el.offsetTop - 8, behavior: "smooth" });
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
                natural={info}
                doc={doc}
                scale={effectiveScale}
                documentId={documentId}
                darkMode={darkMode}
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
  natural: NaturalPage;
  doc: PDFDocumentProxy;
  scale: number;
  documentId: string;
  darkMode: boolean;
  cache: LruCache<HTMLCanvasElement>;
  onMount: (el: HTMLDivElement | null) => void;
}

function PageSlot({
  natural,
  doc,
  scale,
  documentId,
  darkMode,
  cache,
  onMount,
}: SlotProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [visible, setVisible] = useState(false);

  const cacheKey = useMemo(() => {
    const dpr =
      typeof window !== "undefined" ? window.devicePixelRatio || 1 : 1;
    return `${documentId}:${natural.pageNumber}:${scale}:${dpr}:${darkMode ? "d" : "l"}`;
  }, [documentId, natural.pageNumber, scale, darkMode]);

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

    // Always clear before re-populating. Earlier versions only cleared
    // on the !visible branch, which leaked canvases on cacheKey change
    // (theme flip, scale change) because the cleanup ran *after* the
    // next effect populated.
    while (el.firstChild) el.removeChild(el.firstChild);

    if (!visible) return;

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
      pageNumber: natural.pageNumber,
      scale,
      canvas,
    })
      .then(() => {
        if (cancelled) return;
        if (darkMode) invertCanvasForDarkMode(canvas);
        cache.set(cacheKey, canvas);
      })
      .catch((err) => {
        console.warn(
          `page ${natural.pageNumber} render failed:`,
          err instanceof Error ? err.message : err,
        );
      });

    return () => {
      cancelled = true;
    };
  }, [visible, cacheKey, cache, doc, natural.pageNumber, scale, darkMode]);

  return (
    <div
      ref={containerRef}
      data-page={natural.pageNumber}
      style={{
        width: Math.floor(natural.width * scale),
        height: Math.floor(natural.height * scale),
      }}
      className="relative bg-white dark:bg-neutral-100"
    />
  );
}
