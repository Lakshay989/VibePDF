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

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { PDFDocumentProxy } from "pdfjs-dist";

import { deletePages } from "@/ipc/delete-pages";
import { renderPage } from "@/ipc/pdf";
import { rotatePages } from "@/ipc/rotate";
import { deleteThumb, getThumb, putThumb } from "@/panels/thumbnail-cache";
import { useDocEpoch, useEditEpochStore } from "@/state/edit-epoch-store";
import { useHistoryStore } from "@/state/history-store";
import {
  useDocRotations,
  useRotationPreviewStore,
} from "@/state/rotation-preview-store";
import { RotateMenu } from "@/tools/rotate/RotateMenu";
import { DARK_PAGE_FILTER } from "@/view/dark-page-filter";

interface Props {
  doc: PDFDocumentProxy;
  documentId: string;
  onJump: (page: number) => void;
  // SPEC: P1-VIEW-010 — invert thumbnails in dark mode so the sidebar
  // matches the main page view (same `DARK_PAGE_FILTER`).
  darkMode: boolean;
}

/** Target CSS width of a thumbnail. */
const THUMB_CSS_WIDTH = 96;

/** Quantise devicePixelRatio to 1 or 2 so the cache key stays stable
 *  (a 1.5-dpr display and a 2-dpr display share the 2× entry). */
function deviceDpr(): number {
  const raw = typeof window !== "undefined" ? window.devicePixelRatio : 1;
  return (raw ?? 1) >= 2 ? 2 : 1;
}

export function ThumbnailPanel({ doc, documentId, onJump, darkMode }: Props) {
  const pageCount = doc.numPages;
  const dpr = useMemo(() => deviceDpr(), []);

  // Pages that have entered the viewport at least once. Grows only —
  // once a thumbnail is loaded we keep it (the PNGs are small).
  const [visible, setVisible] = useState<ReadonlySet<number>>(new Set());
  const [active, setActive] = useState<number | null>(null);

  // SPEC: P2-PAGE-001 — rotate control state. `menu` is the open context
  // menu (page + screen position). Thumbnail refresh is driven by the
  // shared edit epoch (bumped on any edit/undo/redo), so the sidebar stays
  // in sync with the main view and with undo/redo — not just direct edits.
  const [menu, setMenu] = useState<{ page: number; x: number; y: number } | null>(
    null,
  );
  const epoch = useDocEpoch(documentId);
  const bumpEpoch = useEditEpochStore((s) => s.bumpEpoch);
  const markEdited = useEditEpochStore((s) => s.markEdited);
  const rotations = useDocRotations(documentId);
  const rotatePreview = useRotationPreviewStore((s) => s.rotate);
  const setHistory = useHistoryStore((s) => s.setHistory);

  const scrollRef = useRef<HTMLDivElement>(null);

  // SPEC: P2-PAGE-001 — rotate one page. PDFium holds the real /Rotate; the
  // cosmetic rotation store previews it in the main view *without* a full
  // re-parse (the fast path). We do NOT bump the edit epoch — only update
  // the per-page rotation (the main view + this page's thumbnail react to
  // it) and sync the Undo button. Undo/redo go through the full reload.
  const rotate = useCallback(
    async (page: number, degrees: number) => {
      try {
        const history = await rotatePages(documentId, [page], degrees);
        rotatePreview(documentId, page, degrees);
        // No epoch bump (cosmetic preview) — but mark the doc edited so a
        // later reload (e.g. switching back) loads the rotated actor bytes,
        // not the pristine disk copy.
        markEdited(documentId);
        setHistory(documentId, history);
      } catch (err) {
        console.warn("rotate failed", documentId, page, err);
      }
    },
    [documentId, rotatePreview, markEdited, setHistory],
  );

  // SPEC: P2-PAGE-003 — delete a page, then bump the epoch (refresh both
  // views) and sync the Undo button. Undoable, so no confirm dialog.
  const del = useCallback(
    async (page: number) => {
      try {
        const history = await deletePages(documentId, [page]);
        bumpEpoch(documentId);
        setHistory(documentId, history);
      } catch (err) {
        console.warn("delete failed", documentId, page, err);
      }
    },
    [documentId, bumpEpoch, setHistory],
  );

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
              darkMode={darkMode}
              version={epoch}
              rotation={rotations[page] ?? 0}
              shouldLoad={visible.has(page)}
              isActive={active === page}
              onSelect={() => {
                setActive(page);
                onJump(page + 1);
              }}
              onContextMenu={(x, y) => setMenu({ page, x, y })}
              onDelete={() => void del(page)}
            />
          ))}
        </ul>
      </div>
      {menu ? (
        <RotateMenu
          x={menu.x}
          y={menu.y}
          onRotate={(degrees) => void rotate(menu.page, degrees)}
          onDelete={() => void del(menu.page)}
          onClose={() => setMenu(null)}
        />
      ) : null}
    </aside>
  );
}

interface TileProps {
  doc: PDFDocumentProxy;
  documentId: string;
  page: number;
  dpr: number;
  darkMode: boolean;
  /** Render token; bumped on an edit (delete / reload) to force a re-render. */
  version: number;
  /** Cosmetic rotation (degrees) for this page; a change re-renders the tile. */
  rotation: number;
  shouldLoad: boolean;
  isActive: boolean;
  onSelect: () => void;
  onContextMenu: (x: number, y: number) => void;
  onDelete: () => void;
}

function ThumbTile({
  doc,
  documentId,
  page,
  dpr,
  darkMode,
  version,
  rotation,
  shouldLoad,
  isActive,
  onSelect,
  onContextMenu,
  onDelete,
}: TileProps) {
  const [url, setUrl] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  // The (version, rotation) this tile last loaded, as a string key. Kept in
  // a ref (not deps) so setting `url` doesn't re-run the effect (which would
  // revoke the blob URL we just handed to <img>). The effect re-runs only
  // when the version (an edit) or the rotation changes — then we invalidate
  // the cache and re-render (PDFium applies the real /Rotate).
  const loadedKeyRef = useRef("");

  useEffect(() => {
    const loadKey = `${version}:${rotation}`;
    if (!shouldLoad || loadedKeyRef.current === loadKey) return;
    const isRefresh = loadedKeyRef.current !== ""; // first load isn't a refresh
    loadedKeyRef.current = loadKey;
    let cancelled = false;
    let objectUrl: string | null = null;

    void (async () => {
      try {
        // After an edit, drop the stale cached thumbnail so we re-render.
        if (isRefresh) await deleteThumb({ documentId, page, dpr });
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
          // IPC hands `bytes` back as a plain number[]; make a real
          // Uint8Array before caching/using it.
          png = Uint8Array.from(rendered.bytes);
          void putThumb({ documentId, page, dpr }, png);
        }
        if (cancelled) return;
        // Normalise: a cache hit is a Uint8Array, but a stale entry from
        // the pre-fix build (or any structured-clone quirk) could be a
        // plain array — coerce so `.byteLength` is always defined.
        const u8 = png instanceof Uint8Array ? png : Uint8Array.from(png);
        // Copy into a plain ArrayBuffer-backed buffer so the Blob part
        // is unambiguously `ArrayBuffer` (TS won't widen
        // `Uint8Array<ArrayBufferLike>` to `BlobPart`).
        const ab = new ArrayBuffer(u8.byteLength);
        new Uint8Array(ab).set(u8);
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
  }, [shouldLoad, version, rotation, doc, documentId, page, dpr]);

  return (
    <li className="flex flex-col items-center">
      <button
        type="button"
        onClick={onSelect}
        onContextMenu={(e) => {
          e.preventDefault();
          onContextMenu(e.clientX, e.clientY);
        }}
        onKeyDown={(e) => {
          // SPEC: P2-PAGE-003 — Delete/Backspace removes the focused page
          // (undoable). The tile is focusable, so this only fires when a
          // thumbnail has keyboard focus.
          if (e.key === "Delete" || e.key === "Backspace") {
            e.preventDefault();
            onDelete();
          }
        }}
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
              style={darkMode ? { filter: DARK_PAGE_FILTER } : undefined}
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
