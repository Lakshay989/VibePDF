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
import { AnnotationLayer } from "@/view/annotation-layer";
import { FreeTextLayer } from "@/view/free-text-layer";
import { FormButtonsLayer } from "@/view/form-buttons-layer";
import { FormFieldsLayer } from "@/view/form-fields-layer";
import { TextEditLayer } from "@/view/text-edit-layer";
import { TextBoxLayer } from "@/view/text-box-layer";
import { ImageAddLayer } from "@/view/image-add-layer";
import { ImageEditLayer } from "@/view/image-edit-layer";
import { LinkLayer } from "@/view/link-layer";
import { InkLayer } from "@/view/ink-layer";
import { NoteLayer } from "@/view/note-layer";
import { PolygonLayer } from "@/view/polygon-layer";
import { SelectionHighlightLayer } from "@/view/selection-highlight-layer";
import { MeasureLayer } from "@/view/measure-layer";
import { StampLayer } from "@/view/stamp-layer";
import { PageTextLayer } from "@/view/text-layer";
import { LruCache } from "@/view/page-cache";
import { DARK_PAGE_FILTER } from "@/view/dark-page-filter";
import { useDocRotations } from "@/state/rotation-preview-store";
import type { FitMode } from "@/state/view-store";

// SPEC: P1-VIEW-010 — dark-mode page invert. We apply a CSS filter
// (`DARK_PAGE_FILTER`) to the canvas rather than rewriting its pixels:
// the compositor inverts at native resolution, so text stays exactly as
// crisp as in light mode (a per-pixel getImageData invert looked rough).
// The sidebar thumbnails apply the same filter so the two views agree.

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
  /** Edit epoch; bumped on each edit so cached page renders invalidate. */
  epoch: number;
  /** Page to scroll to once measured (restores position after an edit reload). */
  initialPage: number;
  /** Exact scroll offset (px) to restore after an edit reload — preferred over
   *  `initialPage` so an annotation edit keeps the precise position, not just the
   *  page top. `0`/undefined falls back to the page-based restore. */
  initialScrollTop?: number;
  zoom: number;
  fitMode: FitMode | null;
  darkMode: boolean;
  /** Pinch / Ctrl+wheel zoom: called with the new absolute scale. */
  onZoom?: (scale: number) => void;
  /** Fires when a page's canvas has painted. Used to lift the freeze-frame
   *  overlay the instant the reloaded document's first page is visible. */
  onPageRendered?: (page: number) => void;
}

export interface PageVirtualizerHandle {
  scrollToPage: (page: number) => void;
  scrollByPages: (delta: number) => void;
  scrollByLine: (deltaPx: number) => void;
  getCurrentPage: () => number;
  /** Current scroll offset (px), captured before an edit reload to restore it. */
  getScrollTop: () => number;
  /** A PNG data-URL of the currently-visible pages (drawn from their canvases),
   *  or `null` if nothing is measured. Captured just before an edit reload so a
   *  freeze-frame can bridge the blank while PDF.js re-parses. */
  snapshotVisible: () => string | null;
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
    { doc, documentId, epoch, initialPage, initialScrollTop, zoom, fitMode, darkMode, onZoom, onPageRendered },
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

    // Page to restore once pages are measured. A ref (not a dep) so a late
    // prop change doesn't re-trigger the scroll.
    const initialPageRef = useRef(initialPage);
    initialPageRef.current = initialPage;

    useEffect(() => {
      let cancelled = false;
      (async () => {
        try {
          // Measure in parallel — sequential awaits are O(numPages) round
          // trips, which is painful on large documents.
          const out = await Promise.all(
            Array.from({ length: doc.numPages }, async (_, i) => {
              const page = await doc.getPage(i + 1);
              const v = page.getViewport({ scale: 1 });
              return { pageNumber: i + 1, width: v.width, height: v.height };
            }),
          );
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

    // Restore position once the slots exist (after `pages` commits) — an edit
    // reload remounts us, so we'd otherwise land at the top. Prefer the *exact*
    // scroll offset (page heights + scale are unchanged across an annotation
    // edit, so the px offset maps to the same place); fall back to the page top.
    const initialScrollTopRef = useRef(initialScrollTop ?? 0);
    initialScrollTopRef.current = initialScrollTop ?? 0;
    useEffect(() => {
      if (!pages) return;
      const exact = initialScrollTopRef.current;
      const target = Math.min(Math.max(1, initialPageRef.current), pages.length);
      if (exact <= 0 && target <= 1) return;
      requestAnimationFrame(() => {
        const scroller = scrollRef.current;
        if (!scroller) return;
        if (exact > 0) {
          scroller.scrollTo({ top: exact, behavior: "auto" });
          return;
        }
        const el = slotsRef.current.get(target);
        if (el) {
          scroller.scrollTo({ top: el.offsetTop - 8, behavior: "auto" });
          currentPageRef.current = target;
        }
      });
    }, [pages]);

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

    // Cosmetic per-page rotations (the rotate fast-path). Apply them to the
    // measured dimensions — a 90°/270° page is laid out (and rendered)
    // landscape — so the view reflects a rotation without re-parsing.
    const rotations = useDocRotations(documentId);
    const effectivePages = useMemo(() => {
      if (!pages) return null;
      return pages.map((p) => {
        const rotation = rotations[p.pageNumber - 1] ?? 0;
        const swap = rotation % 180 === 90;
        return {
          pageNumber: p.pageNumber,
          width: swap ? p.height : p.width,
          height: swap ? p.width : p.height,
          rotation,
        };
      });
    }, [pages, rotations]);

    const effectiveScale = useMemo(() => {
      if (!effectivePages || effectivePages.length === 0) return zoom;
      if (!fitMode) return zoom;
      if (!containerSize) return zoom;
      const first = effectivePages[0];
      return computeFitScale(
        fitMode,
        containerSize.w,
        containerSize.h,
        first.width,
        first.height,
      );
    }, [effectivePages, zoom, fitMode, containerSize]);

    // Drop bitmap cache whenever the effective scale, theme, or edit
    // epoch changes. All three are in the per-slot cache key, so stale
    // entries would never be re-used; clearing also frees memory promptly.
    // (Edit epoch matters: a 180° rotate leaves page dimensions unchanged,
    // so without it a cached render would be served for the rotated page.)
    useEffect(() => {
      cacheRef.current.clear();
    }, [effectiveScale, darkMode, epoch]);

    // Pinch-zoom (a trackpad pinch arrives as a Ctrl+wheel event on macOS)
    // and Ctrl/Cmd+wheel. A non-passive listener so we can preventDefault
    // the webview's own page zoom. Reads the live scale via a ref so the
    // listener doesn't re-attach on every scale change.
    const effectiveScaleRef = useRef(effectiveScale);
    effectiveScaleRef.current = effectiveScale;
    useEffect(() => {
      const el = scrollRef.current;
      if (!el || !onZoom) return;
      const onWheel = (e: WheelEvent) => {
        if (!(e.ctrlKey || e.metaKey)) return;
        e.preventDefault();
        // Clamp the per-event delta so a mouse-wheel notch (deltaY ~100)
        // doesn't lurch, while a trackpad pinch (small deltas, many events)
        // stays smooth. Update the ref *immediately* so a rapid burst of
        // events accumulates instead of all reading the same pre-render
        // scale (that staleness made zooming crawl).
        const d = Math.max(-40, Math.min(40, e.deltaY));
        const next = effectiveScaleRef.current * Math.exp(-d * 0.01);
        effectiveScaleRef.current = next;
        onZoom(next);
      };
      el.addEventListener("wheel", onWheel, { passive: false });
      return () => el.removeEventListener("wheel", onWheel);
    }, [onZoom]);

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
        getScrollTop: () => scrollRef.current?.scrollTop ?? 0,
        snapshotVisible: () => {
          const scroller = scrollRef.current;
          if (!scroller) return null;
          const w = scroller.clientWidth;
          const h = scroller.clientHeight;
          if (w === 0 || h === 0) return null;
          const dpr = window.devicePixelRatio || 1;
          const snap = document.createElement("canvas");
          snap.width = Math.floor(w * dpr);
          snap.height = Math.floor(h * dpr);
          const ctx = snap.getContext("2d");
          if (!ctx) return null;
          ctx.scale(dpr, dpr);
          // Fill the scroller's own background so page gaps aren't transparent.
          ctx.fillStyle = window.getComputedStyle(scroller).backgroundColor || "#f5f5f5";
          ctx.fillRect(0, 0, w, h);
          // Dark mode displays the page canvases through a CSS invert filter; match
          // it so the frozen frame doesn't flash light. (If the WebView ignores
          // `ctx.filter`, the snapshot is un-inverted — a sub-second cosmetic miss.)
          if (darkMode) ctx.filter = DARK_PAGE_FILTER;
          const viewport = scroller.getBoundingClientRect();
          for (const canvas of scroller.querySelectorAll("canvas")) {
            const r = canvas.getBoundingClientRect();
            if (r.bottom < viewport.top || r.top > viewport.bottom) continue; // offscreen
            try {
              ctx.drawImage(canvas, r.left - viewport.left, r.top - viewport.top, r.width, r.height);
            } catch {
              // A tainted/unreadable canvas can't be drawn — skip it, keep the rest.
            }
          }
          return snap.toDataURL();
        },
      }),
      [pages, darkMode],
    );

    const registerSlot = (page: number, el: HTMLDivElement | null) => {
      if (el) slotsRef.current.set(page, el);
      else slotsRef.current.delete(page);
    };

    return (
      <div
        ref={scrollRef}
        // `relative`: make this scroller the offsetParent for the page slots, so
        // `el.offsetTop` is measured from the scroll content's top (not from a
        // positioned ancestor that includes the toolbar above us). Without it,
        // jump-to-page over-scrolls by the toolbar's height — pages landed ~20%
        // down — and the current-page tracking was skewed by the same constant.
        className="relative h-full overflow-auto bg-neutral-100 dark:bg-neutral-900"
        tabIndex={0}
      >
        {error ? (
          <div className="mx-auto mt-8 max-w-lg p-4 text-sm text-red-700 dark:text-red-300">
            {error}
          </div>
        ) : !effectivePages ? (
          <div className="p-4 text-sm text-neutral-500">Reading pages…</div>
        ) : (
          <div className="flex flex-col items-center gap-4 py-4">
            {effectivePages.map((info) => (
              <PageSlot
                key={info.pageNumber}
                natural={info}
                rotation={info.rotation}
                doc={doc}
                scale={effectiveScale}
                documentId={documentId}
                epoch={epoch}
                darkMode={darkMode}
                cache={cacheRef.current}
                onMount={(el) => registerSlot(info.pageNumber, el)}
                onRendered={onPageRendered}
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
  rotation: number;
  doc: PDFDocumentProxy;
  scale: number;
  documentId: string;
  epoch: number;
  darkMode: boolean;
  cache: LruCache<HTMLCanvasElement>;
  onMount: (el: HTMLDivElement | null) => void;
  /** Fired once this page's canvas is on screen (fresh render or cache hit). */
  onRendered?: ((page: number) => void) | undefined;
}

function PageSlot({
  natural,
  rotation,
  doc,
  scale,
  documentId,
  epoch,
  darkMode,
  cache,
  onMount,
  onRendered,
}: SlotProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  // The canvas mounts into its own inner div (cleared imperatively) so the React
  // annotation overlay — a sibling — survives that clear. `containerRef` stays
  // the outer flow element registered for scroll (its `offsetTop` drives
  // jump-to-page) and observed for visibility.
  const canvasRef = useRef<HTMLDivElement>(null);
  const [visible, setVisible] = useState(false);
  // Latest `onRendered` without making it a render-effect dep (which would force
  // a needless re-rasterize whenever the parent re-creates the callback).
  const onRenderedRef = useRef(onRendered);
  onRenderedRef.current = onRendered;

  const cacheKey = useMemo(() => {
    const dpr =
      typeof window !== "undefined" ? window.devicePixelRatio || 1 : 1;
    return `${documentId}:${epoch}:${natural.pageNumber}:${rotation}:${scale}:${dpr}:${darkMode ? "d" : "l"}`;
  }, [documentId, epoch, natural.pageNumber, rotation, scale, darkMode]);

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
    const el = canvasRef.current;
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
      onRenderedRef.current?.(natural.pageNumber);
      return;
    }

    let cancelled = false;
    const canvas = document.createElement("canvas");
    canvas.style.maxWidth = "100%";
    // Display-only invert in dark mode; the bitmap stays pristine, and
    // the style survives caching (cacheKey includes darkMode).
    if (darkMode) canvas.style.filter = DARK_PAGE_FILTER;
    canvas.className = "shadow";
    el.appendChild(canvas);

    void renderPageOnDoc({
      doc,
      pageNumber: natural.pageNumber,
      scale,
      canvas,
      rotation,
    })
      .then(() => {
        if (cancelled) return;
        cache.set(cacheKey, canvas);
        onRenderedRef.current?.(natural.pageNumber);
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
  }, [visible, cacheKey, cache, doc, natural.pageNumber, rotation, scale, darkMode]);

  // Unrotated PDF dimensions, exposed as data-* so the text-markup apply path
  // can reconstruct each page's geometry from the DOM (see tools/text-markup).
  const swapped = (((rotation % 180) + 180) % 180) === 90;
  return (
    <div
      ref={containerRef}
      data-page={natural.pageNumber}
      data-pdf-w={swapped ? natural.height : natural.width}
      data-pdf-h={swapped ? natural.width : natural.height}
      data-rotation={rotation}
      style={{
        width: Math.floor(natural.width * scale),
        height: Math.floor(natural.height * scale),
      }}
      className="relative bg-white dark:bg-neutral-100"
    >
      <div ref={canvasRef} className="absolute inset-0" />
      {visible ? (
        <PageTextLayer
          doc={doc}
          pageNumber={natural.pageNumber}
          scale={scale}
          rotation={rotation}
        />
      ) : null}
      <AnnotationLayer
        documentId={documentId}
        page={natural.pageNumber - 1}
        displayedWidth={natural.width}
        displayedHeight={natural.height}
        scale={scale}
        rotation={rotation}
      />
      <NoteLayer
        documentId={documentId}
        page={natural.pageNumber - 1}
        displayedWidth={natural.width}
        displayedHeight={natural.height}
        scale={scale}
        rotation={rotation}
      />
      <FreeTextLayer
        documentId={documentId}
        page={natural.pageNumber - 1}
        displayedWidth={natural.width}
        displayedHeight={natural.height}
        scale={scale}
        rotation={rotation}
      />
      <TextEditLayer
        documentId={documentId}
        page={natural.pageNumber - 1}
        displayedWidth={natural.width}
        displayedHeight={natural.height}
        scale={scale}
        rotation={rotation}
      />
      <FormFieldsLayer
        documentId={documentId}
        page={natural.pageNumber - 1}
        displayedWidth={natural.width}
        displayedHeight={natural.height}
        scale={scale}
        rotation={rotation}
      />
      <FormButtonsLayer
        documentId={documentId}
        page={natural.pageNumber - 1}
        displayedWidth={natural.width}
        displayedHeight={natural.height}
        scale={scale}
        rotation={rotation}
      />
      <TextBoxLayer
        documentId={documentId}
        page={natural.pageNumber - 1}
        displayedWidth={natural.width}
        displayedHeight={natural.height}
        scale={scale}
        rotation={rotation}
      />
      <ImageAddLayer
        documentId={documentId}
        page={natural.pageNumber - 1}
        displayedWidth={natural.width}
        displayedHeight={natural.height}
        scale={scale}
        rotation={rotation}
      />
      <ImageEditLayer
        documentId={documentId}
        page={natural.pageNumber - 1}
        displayedWidth={natural.width}
        displayedHeight={natural.height}
        scale={scale}
        rotation={rotation}
      />
      <LinkLayer
        documentId={documentId}
        page={natural.pageNumber - 1}
        pageCount={doc.numPages}
        displayedWidth={natural.width}
        displayedHeight={natural.height}
        scale={scale}
        rotation={rotation}
      />
      <PolygonLayer
        documentId={documentId}
        page={natural.pageNumber - 1}
        displayedWidth={natural.width}
        displayedHeight={natural.height}
        scale={scale}
        rotation={rotation}
      />
      <InkLayer
        documentId={documentId}
        page={natural.pageNumber - 1}
        displayedWidth={natural.width}
        displayedHeight={natural.height}
        scale={scale}
        rotation={rotation}
      />
      <StampLayer
        documentId={documentId}
        page={natural.pageNumber - 1}
        displayedWidth={natural.width}
        displayedHeight={natural.height}
        scale={scale}
        rotation={rotation}
      />
      <MeasureLayer
        documentId={documentId}
        page={natural.pageNumber - 1}
        displayedWidth={natural.width}
        displayedHeight={natural.height}
        scale={scale}
        rotation={rotation}
      />
      <SelectionHighlightLayer
        page={natural.pageNumber - 1}
        displayedWidth={natural.width}
        displayedHeight={natural.height}
        scale={scale}
        rotation={rotation}
      />
    </div>
  );
}
