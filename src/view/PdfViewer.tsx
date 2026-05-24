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
import { useDarkMode } from "@/app/use-dark-mode";
import { useViewStore } from "@/state/view-store";
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
  const darkMode = useDarkMode();

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

  // SPEC: P1-VIEW-005 (P1.C3) + P1-VIEW-006 (P1.C2) — keyboard router.
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      const cmd = e.metaKey || e.ctrlKey;
      const inputFocused = isInputFocused(document.activeElement);

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
  }, [doc, zoom, setZoom, setFitMode]);

  return (
    <div className="flex h-full flex-col">
      <ZoomToolbar />
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
  );
}
