// SPEC: P1-VIEW-008 (P1.D1) — collapsible thumbnails sidebar with lazy
// generation.
//
// One tile per page, all mounted as lightweight placeholders. A single
// shared IntersectionObserver watches them; when a tile first enters the
// viewport we render its page to a ~96px-wide PNG via the backend
// (PDFium through `pdf_render_page`), cache it in IndexedDB keyed by
// (documentId, page, dpr), and show it. Cached tiles return as an
// instant IDB hit.
//
// Thumbnails render via PDFium on the Rust side (per docs/04 — PDFium
// owns thumbnail rasterisation); the frontend PDF.js `doc` is used only
// to read each page's point-width so we can compute the DPI that yields
// ~96 CSS px. The panel is keyed by documentId at the mount site, so a
// document switch remounts it with a fresh observer + empty state.

import { useEffect, useMemo, useRef, useState } from "react";
import type { PDFDocumentProxy } from "pdfjs-dist";

import { renderPage } from "@/ipc/pdf";
import { getThumb, putThumb } from "@/panels/thumbnail-cache";

interface Props {
  doc: PDFDocumentProxy;
  documentId: string;
  onJump: (page: number) => void;
}

/** Target CSS width of a thumbnail. */
const THUMB_CSS_WIDTH = 96;

/** Quantise devicePixelRatio to 1 or 2 so the cache key stays stable
 *  (a 1.5-dpr display and a 2-dpr display share the 2× entry). */
function deviceDpr(): number {
  const raw = typeof window !== "undefined" ? window.devicePixelRatio : 1;
  return (raw ?? 1) >= 2 ? 2 : 1;
}

export function ThumbnailPanel({ doc, documentId, onJump }: Props) {
  const pageCount = doc.numPages;
  const dpr = useMemo(() => deviceDpr(), []);

  // Pages that have entered the viewport at least once. Grows only —
  // once a thumbnail is loaded we keep it (the PNGs are small).
  const [visible, setVisible] = useState<ReadonlySet<number>>(new Set());
  const [active, setActive] = useState<number | null>(null);

  const scrollRef = useRef<HTMLDivElement>(null);

  // One shared observer for all tiles. All tiles mount at once (fixed
  // pageCount, no virtualization), so we can observe them straight from
  // the DOM after commit — no per-tile callback refs / timing dance.
  useEffect(() => {
    const root = scrollRef.current;
    if (!root) return;
    const obs = new IntersectionObserver(
      (entries) => {
        const newly: number[] = [];
        for (const entry of entries) {
          if (!entry.isIntersecting) continue;
          const page = Number((entry.target as HTMLElement).dataset.thumbPage);
          if (!Number.isNaN(page)) {
            newly.push(page);
            obs.unobserve(entry.target); // load once
          }
        }
        if (newly.length > 0) {
          setVisible((prev) => {
            const next = new Set(prev);
            for (const p of newly) next.add(p);
            return next;
          });
        }
      },
      { root, rootMargin: "200px 0px" },
    );
    root
      .querySelectorAll<HTMLElement>("[data-thumb-page]")
      .forEach((el) => obs.observe(el));
    return () => obs.disconnect();
  }, [pageCount]);

  return (
    <aside className="flex h-full w-32 flex-col border-r border-neutral-200 bg-neutral-50 dark:border-neutral-800 dark:bg-neutral-950">
      <header className="border-b border-neutral-200 px-3 py-2 text-xs font-semibold uppercase tracking-wide text-neutral-500 dark:border-neutral-800">
        Pages
        <span className="ml-1 text-neutral-400">({pageCount})</span>
      </header>
      <div ref={scrollRef} className="flex-1 overflow-auto p-2">
        <ul className="flex flex-col items-center gap-2">
          {Array.from({ length: pageCount }, (_, page) => (
            <ThumbTile
              key={page}
              doc={doc}
              documentId={documentId}
              page={page}
              dpr={dpr}
              shouldLoad={visible.has(page)}
              isActive={active === page}
              onSelect={() => {
                setActive(page);
                onJump(page + 1);
              }}
            />
          ))}
        </ul>
      </div>
    </aside>
  );
}

interface TileProps {
  doc: PDFDocumentProxy;
  documentId: string;
  page: number;
  dpr: number;
  shouldLoad: boolean;
  isActive: boolean;
  onSelect: () => void;
}

function ThumbTile({
  doc,
  documentId,
  page,
  dpr,
  shouldLoad,
  isActive,
  onSelect,
}: TileProps) {
  const [url, setUrl] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  // Load-once guard. Kept in a ref (not the deps) so setting `url`
  // doesn't re-run this effect — which would fire the cleanup and
  // revoke the blob URL we just handed to <img>. The effect's deps are
  // all stable for a tile's lifetime once `shouldLoad` flips true, so
  // the cleanup runs only on real unmount.
  const startedRef = useRef(false);

  useEffect(() => {
    if (!shouldLoad || startedRef.current) return;
    startedRef.current = true;
    let cancelled = false;
    let objectUrl: string | null = null;

    void (async () => {
      try {
        let png = await getThumb({ documentId, page, dpr });
        if (!png) {
          // Compute the DPI that renders this page at ~THUMB_CSS_WIDTH·dpr
          // px wide. PDF.js gives the page's width in points (1/72").
          const pdfPage = await doc.getPage(page + 1);
          const widthPt = pdfPage.getViewport({ scale: 1 }).width;
          const targetPx = THUMB_CSS_WIDTH * dpr;
          const dpi = (targetPx * 72) / widthPt;
          const rendered = await renderPage(documentId, page, dpi, "png");
          if (cancelled) return;
          png = rendered.bytes;
          void putThumb({ documentId, page, dpr }, png);
        }
        if (cancelled) return;
        // Copy into a plain ArrayBuffer-backed buffer: `renderPage`
        // returns `Uint8Array<ArrayBufferLike>`, which TS won't widen to
        // `BlobPart` (the backing buffer could in principle be a
        // SharedArrayBuffer). A small copy sidesteps the variance.
        const ab = new ArrayBuffer(png.byteLength);
        new Uint8Array(ab).set(png);
        objectUrl = URL.createObjectURL(new Blob([ab], { type: "image/png" }));
        setUrl(objectUrl);
      } catch (err) {
        if (!cancelled) {
          console.warn("thumbnail render failed", documentId, page, err);
          setFailed(true);
        }
      }
    })();

    return () => {
      cancelled = true;
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [shouldLoad, doc, documentId, page, dpr]);

  return (
    <li className="flex flex-col items-center">
      <button
        type="button"
        onClick={onSelect}
        title={`Page ${page + 1}`}
        aria-label={`Go to page ${page + 1}`}
        aria-current={isActive ? "true" : undefined}
        className={
          "block overflow-hidden rounded border bg-white shadow-sm dark:bg-neutral-800 " +
          (isActive
            ? "border-blue-500 ring-1 ring-blue-500"
            : "border-neutral-300 hover:border-neutral-400 dark:border-neutral-700")
        }
      >
        <div
          data-thumb-page={page}
          className="flex h-32 w-24 items-center justify-center text-neutral-300 dark:text-neutral-600"
        >
          {url ? (
            <img
              src={url}
              alt={`Page ${page + 1}`}
              className="h-full w-full object-contain"
            />
          ) : failed ? (
            <span className="text-xs">⚠</span>
          ) : (
            <span className="text-xs tabular-nums">{page + 1}</span>
          )}
        </div>
      </button>
      <span className="mt-0.5 text-xs tabular-nums text-neutral-400">
        {page + 1}
      </span>
    </li>
  );
}
