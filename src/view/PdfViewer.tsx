import { useEffect, useRef, useState } from "react";
import { readFile } from "@tauri-apps/plugin-fs";
import type { PDFDocumentProxy } from "pdfjs-dist";

import { loadDocument } from "@/view/render-page";
import {
  PageVirtualizer,
  type PageVirtualizerHandle,
} from "@/view/PageVirtualizer";
import { isInputFocused, keyToIntent } from "@/view/keyboard-nav";
import { ZoomToolbar } from "@/app/ZoomToolbar";
import { SearchBar } from "@/app/SearchBar";
import { useDarkMode } from "@/app/use-dark-mode";
import { OutlinePanel } from "@/panels/OutlinePanel";
import { ThumbnailPanel } from "@/panels/ThumbnailPanel";
import { useViewStore } from "@/state/view-store";
import { useSearchStore } from "@/state/search-store";
import { searchDoc } from "@/view/search";
import {
  loadViewSettings,
  pathHash,
  saveViewSettings,
} from "@/state/view-persistence";
import type { DocumentId } from "@/ipc/pdf";

interface Props {
  documentId: DocumentId;
  path: string;
}

// SPEC: P1-VIEW-001, P1-VIEW-005, P1-VIEW-006, NFR-PERF-003.
//
// PdfViewer owns:
//  - the document lifecycle (load once, destroy on unmount)
//  - the per-doc view persistence (path-hashed IDB get/set)
//  - the keyboard nav + zoom shortcut router
//
// PageVirtualizer owns rendering, scrolling, and the imperative API.
export function PdfViewer({ documentId, path }: Props) {
  const [doc, setDoc] = useState<PDFDocumentProxy | null>(null);
  const [error, setError] = useState<string | null>(null);
  const virtRef = useRef<PageVirtualizerHandle>(null);

  const zoom = useViewStore((s) => s.zoom);
  const fitMode = useViewStore((s) => s.fitMode);
  const setView = useViewStore((s) => s.setView);
  const setZoom = useViewStore((s) => s.setZoom);
  const setFitMode = useViewStore((s) => s.setFitMode);
  const showOutline = useViewStore((s) => s.showOutline);
  const showThumbnails = useViewStore((s) => s.showThumbnails);
  const darkMode = useDarkMode();

  // Search state subscriptions (kept narrow to avoid extra renders).
  const searchOpen = useSearchStore((s) => s.isOpen);
  const searchQuery = useSearchStore((s) => s.query);
  const searchCase = useSearchStore((s) => s.caseSensitive);
  const searchWhole = useSearchStore((s) => s.wholeWord);
  const searchFlat = useSearchStore((s) => s.flat);
  const searchIndex = useSearchStore((s) => s.currentIndex);
  const openSearch = useSearchStore((s) => s.open);
  const closeSearch = useSearchStore((s) => s.close);
  const setMatches = useSearchStore((s) => s.setMatches);
  const setSearching = useSearchStore((s) => s.setSearching);

  // Load document bytes + parse PDF.
  useEffect(() => {
    let cancelled = false;
    let loaded: PDFDocumentProxy | null = null;
    (async () => {
      try {
        const bytes = await readFile(path);
        if (cancelled) return;
        loaded = await loadDocument(bytes);
        if (cancelled) {
          await loaded.destroy();
          return;
        }
        setDoc(loaded);
      } catch (e) {
        const msg =
          e instanceof Error ? e.message : "Failed to open this file as a PDF.";
        if (!cancelled) setError(msg);
      }
    })();
    return () => {
      cancelled = true;
      void loaded?.destroy();
      setDoc(null);
    };
  }, [path]);

  // SPEC: P1-VIEW-006 — restore persisted zoom + fit-mode on open.
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const hash = await pathHash(path);
        const saved = await loadViewSettings(hash);
        if (cancelled || !saved) return;
        setView({ zoom: saved.zoom, fitMode: saved.fitMode });
      } catch (e) {
        console.warn("view-settings load failed:", e);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [path, setView]);

  // SPEC: P1-VIEW-006 — persist zoom + fit-mode on every change.
  // Debounced lightly so a slider-drag doesn't hammer IDB.
  useEffect(() => {
    let cancelled = false;
    const handle = window.setTimeout(() => {
      void (async () => {
        try {
          const hash = await pathHash(path);
          if (cancelled) return;
          await saveViewSettings(hash, { zoom, fitMode });
        } catch (e) {
          console.warn("view-settings save failed:", e);
        }
      })();
    }, 200);
    return () => {
      cancelled = true;
      window.clearTimeout(handle);
    };
  }, [path, zoom, fitMode]);

  // SPEC: P1-VIEW-005 (P1.C3) + P1-VIEW-006 (P1.C2) + P1-VIEW-007 (P1.C4)
  // — keyboard router.
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      const cmd = e.metaKey || e.ctrlKey;
      const inputFocused = isInputFocused(document.activeElement);

      // Cmd/Ctrl+F → open the search bar (always honored, even if an
      // input is focused — Cmd+F should escape any text field).
      if (cmd && e.key.toLowerCase() === "f") {
        e.preventDefault();
        openSearch();
        return;
      }

      // Zoom shortcuts: Cmd/Ctrl + = / + → zoom in; − → out; 0 → fit page.
      if (cmd && !inputFocused) {
        if (e.key === "=" || e.key === "+") {
          e.preventDefault();
          setZoom(zoom + 0.25);
          return;
        }
        if (e.key === "-" || e.key === "_") {
          e.preventDefault();
          setZoom(zoom - 0.25);
          return;
        }
        if (e.key === "0") {
          e.preventDefault();
          setFitMode("fit-page");
          return;
        }
      }

      const intent = keyToIntent(
        {
          key: e.key,
          shiftKey: e.shiftKey,
          ctrlKey: e.ctrlKey,
          metaKey: e.metaKey,
          altKey: e.altKey,
        },
        { inputFocused },
      );
      if (!intent) return;
      const v = virtRef.current;
      if (!v) return;
      e.preventDefault();
      switch (intent.kind) {
        case "page-delta":
          v.scrollByPages(intent.delta);
          break;
        case "page-target":
          v.scrollToPage(intent.page === "first" ? 1 : Number.MAX_SAFE_INTEGER);
          break;
        case "line-delta":
          v.scrollByLine(intent.delta);
          break;
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [doc, zoom, setZoom, setFitMode, openSearch]);

  // SPEC: P1-VIEW-007 — run the search whenever query/options change.
  // Debounced 200 ms so typing doesn't fire one search per keystroke.
  useEffect(() => {
    if (!doc) return;
    if (!searchQuery) {
      setMatches([]);
      setSearching(false);
      return;
    }
    const signal = { cancelled: false };
    const timer = window.setTimeout(() => {
      setSearching(true);
      void searchDoc(doc, searchQuery, {
        caseSensitive: searchCase,
        wholeWord: searchWhole,
      }, signal)
        .then((matches) => {
          if (signal.cancelled) return;
          setMatches(matches);
        })
        .catch((err) => {
          console.warn("search failed:", err);
        })
        .finally(() => {
          if (!signal.cancelled) setSearching(false);
        });
    }, 200);
    return () => {
      signal.cancelled = true;
      window.clearTimeout(timer);
    };
  }, [doc, searchQuery, searchCase, searchWhole, setMatches, setSearching]);

  // SPEC: P1-VIEW-007 — scroll to the page of the current match.
  useEffect(() => {
    if (searchIndex < 0) return;
    const match = searchFlat[searchIndex];
    if (!match) return;
    virtRef.current?.scrollToPage(match.pageNumber);
  }, [searchIndex, searchFlat]);

  // Closing the search bar (via the X or Escape inside the input) is
  // handled by the store. We additionally close on Escape pressed
  // outside the input — same handler tree:
  useEffect(() => {
    if (!searchOpen) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        closeSearch();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [searchOpen, closeSearch]);

  return (
    <div className="flex h-full flex-col">
      <ZoomToolbar />
      <SearchBar />
      <div className="flex flex-1 overflow-hidden">
        {showThumbnails && doc ? (
          <ThumbnailPanel
            key={documentId}
            doc={doc}
            documentId={documentId}
            onJump={(page) => virtRef.current?.scrollToPage(page)}
          />
        ) : null}
        {showOutline && doc ? (
          <OutlinePanel
            doc={doc}
            onJump={(page) => virtRef.current?.scrollToPage(page)}
          />
        ) : null}
        <div className="flex-1 overflow-hidden">
          {error ? (
            <div className="mx-auto mt-8 max-w-lg rounded border border-red-300 bg-red-50 p-3 text-sm text-red-900 dark:border-red-800 dark:bg-red-950 dark:text-red-200">
              This file does not appear to be a valid PDF.
              <div className="mt-1 text-xs opacity-70">{error}</div>
            </div>
          ) : doc ? (
            <PageVirtualizer
              ref={virtRef}
              doc={doc}
              documentId={documentId}
              zoom={zoom}
              fitMode={fitMode}
              darkMode={darkMode}
            />
          ) : (
            <div className="p-4 text-sm text-neutral-500">Opening…</div>
          )}
        </div>
      </div>
    </div>
  );
}
