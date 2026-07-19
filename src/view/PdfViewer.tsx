import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { readFile } from "@tauri-apps/plugin-fs";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import type { PDFDocumentProxy } from "pdfjs-dist";

import { loadDocument } from "@/view/render-page";
import {
  PageVirtualizer,
  type PageVirtualizerHandle,
} from "@/view/PageVirtualizer";
import { isInputFocused, keyToIntent } from "@/view/keyboard-nav";
import { basename } from "@/app/paths";
import { reportError } from "@/app/report-error";
import { ZoomToolbar } from "@/app/ZoomToolbar";
import { MarkupToolbar } from "@/app/MarkupToolbar";
import { ExtractDialog } from "@/app/ExtractDialog";
import { SplitDialog } from "@/app/SplitDialog";
import { MergeDialog } from "@/app/MergeDialog";
import { InsertFromDialog } from "@/app/InsertFromDialog";
import { WatermarkDialog } from "@/app/WatermarkDialog";
import { BackgroundDialog } from "@/app/BackgroundDialog";
import { HeaderFooterDialog } from "@/app/HeaderFooterDialog";
import { SearchBar } from "@/app/SearchBar";
import { FontFallbackBanner } from "@/app/FontFallbackBanner";
import { useFontReport } from "@/app/use-font-report";
import { extractPages } from "@/ipc/extract";
import { splitDocument, type SplitMode } from "@/ipc/split";
import { mergeDocuments } from "@/ipc/merge";
import { insertPagesFromPdf } from "@/ipc/insert-from";
import { useHistoryStore } from "@/state/history-store";
import { useDarkMode } from "@/app/use-dark-mode";
import { AnnotationPanel } from "@/panels/AnnotationPanel";
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
import { isDocEdited, useDocEpoch, useEditEpochStore } from "@/state/edit-epoch-store";
import { useRotationPreviewStore } from "@/state/rotation-preview-store";
import { getPdfBytes, type DocumentId } from "@/ipc/pdf";

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
  const showAnnotations = useViewStore((s) => s.showAnnotations);
  const darkMode = useDarkMode();

  // SPEC: edit-preview pipeline — bumped on every edit/undo/redo; drives
  // the reload-from-actor-bytes effect below.
  const epoch = useDocEpoch(documentId);

  // SPEC: P4-EDIT-002 (P4.A2) — once-per-document warning when a font isn't
  // embedded or installed, so the user knows editing it will substitute.
  const fontReport = useFontReport(documentId);

  // SPEC: P2-PAGE-006 — extract pages: pick a range, then a native save
  // dialog, then write the new PDF. Read-only on the open document.
  const [extractOpen, setExtractOpen] = useState(false);
  const handleExtract = useCallback(
    async (pages: number[]) => {
      setExtractOpen(false);
      try {
        const dest = await saveDialog({
          defaultPath: "extracted.pdf",
          filters: [{ name: "PDF", extensions: ["pdf"] }],
        });
        if (typeof dest !== "string") return; // user cancelled the dialog
        await extractPages(documentId, pages, dest);
      } catch (err) {
        reportError("Couldn't extract pages", err);
      }
    },
    [documentId],
  );

  // SPEC: P2-PAGE-007 — split: pick a mode, then a directory, then write N
  // numbered files. Read-only on the open document. The output stem is the
  // source file name (without extension), defaulting to "split".
  const [splitOpen, setSplitOpen] = useState(false);
  const handleSplit = useCallback(
    async (mode: SplitMode) => {
      setSplitOpen(false);
      try {
        const destDir = await openDialog({ directory: true, multiple: false });
        if (typeof destDir !== "string") return; // user cancelled the dialog
        const base = basename(path);
        const stem = base.replace(/\.pdf$/i, "") || "split";
        await splitDocument(documentId, mode, destDir, stem);
      } catch (err) {
        reportError("Couldn't split the PDF", err);
      }
    },
    [documentId, path],
  );

  // SPEC: P2-PAGE-008 — merge: pick an ordered set of PDFs (seeded with the
  // current file), then a save dialog, then write a new combined PDF. Read-only
  // on every input; the open document is untouched.
  const [mergeOpen, setMergeOpen] = useState(false);
  // Stable reference so MergeDialog's reset effect doesn't re-fire each render.
  const mergeSeedPaths = useMemo(() => [path], [path]);
  const handleMerge = useCallback(
    async (paths: string[]) => {
      setMergeOpen(false);
      try {
        const dest = await saveDialog({
          defaultPath: "merged.pdf",
          filters: [{ name: "PDF", extensions: ["pdf"] }],
        });
        if (typeof dest !== "string") return; // user cancelled the dialog
        await mergeDocuments(paths, dest);
      } catch (err) {
        reportError("Couldn't merge PDFs", err);
      }
    },
    [],
  );

  // SPEC: P2-PAGE-005 — insert pages from another PDF into the open document.
  // A page-tree change, so bump the epoch (full reload, like delete/insert) and
  // sync the undo/redo state. Undoable.
  const bumpEpoch = useEditEpochStore((s) => s.bumpEpoch);
  const setHistory = useHistoryStore((s) => s.setHistory);
  const [insertFromOpen, setInsertFromOpen] = useState(false);
  const [watermarkOpen, setWatermarkOpen] = useState(false);
  const [backgroundOpen, setBackgroundOpen] = useState(false);
  const [headerFooterOpen, setHeaderFooterOpen] = useState(false);
  const handleInsertFromPdf = useCallback(
    async ({
      sourcePath,
      pages,
      index,
    }: {
      sourcePath: string;
      pages: number[];
      index: number;
    }) => {
      setInsertFromOpen(false);
      try {
        const history = await insertPagesFromPdf(documentId, sourcePath, pages, index);
        bumpEpoch(documentId);
        setHistory(documentId, history);
      } catch (err) {
        reportError("Couldn't insert pages", err);
      }
    },
    [documentId, bumpEpoch, setHistory],
  );

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

  // SPEC: edit-preview pipeline — (re)load + parse the PDF. At epoch 0 the
  // bytes come from disk; after an edit (epoch > 0) from the actor's live
  // in-memory document, so the view reflects edits without a save/reopen.
  //
  // We clear `doc` first on *every* (re)load so PageVirtualizer unmounts
  // and remounts cleanly. An in-place doc swap proved unreliable under
  // StrictMode (stale pages, destroyed-doc-still-shown → "invalid pdf").
  // Each effect run owns exactly one document and destroys it on cleanup —
  // no shared ref, no cross-run destroy races. For an edit reload of the
  // *same* document we remember the page to restore (PageVirtualizer
  // scrolls there once measured). The full re-parse per edit is a known
  // cost — incremental preview is tracked in BACKLOG.
  const lastDocIdRef = useRef<string | null>(null);
  const initialPageRef = useRef(1);
  // Exact scroll offset to restore across an edit reload (same doc, same scale,
  // same page heights → the px offset lands in the same place). Avoids the
  // jump-to-page-top the page-granular restore caused on every annotation edit.
  const initialScrollTopRef = useRef(0);
  const resetRotations = useRotationPreviewStore((s) => s.resetDoc);

  useEffect(() => {
    let cancelled = false;
    let localDoc: PDFDocumentProxy | null = null;

    const sameDoc = lastDocIdRef.current === documentId;
    initialPageRef.current = sameDoc
      ? (virtRef.current?.getCurrentPage() ?? 1)
      : 1;
    initialScrollTopRef.current = sameDoc ? (virtRef.current?.getScrollTop() ?? 0) : 0;
    lastDocIdRef.current = documentId;
    setDoc(null);
    // The (re)loaded bytes carry the real /Rotate, so any cosmetic rotation
    // preview for this doc starts over at 0.
    resetRotations(documentId);

    (async () => {
      try {
        // A pristine document loads from disk (cheap); an edited one — even
        // a rotate, which doesn't bump the epoch — must load from the
        // actor's live bytes, which carry the in-memory edits.
        const bytes = isDocEdited(documentId)
          ? await getPdfBytes(documentId)
          : await readFile(path);
        if (cancelled) return;
        localDoc = await loadDocument(bytes);
        if (cancelled) {
          await localDoc.destroy();
          return;
        }
        setDoc(localDoc);
        setError(null);
      } catch (e) {
        const msg =
          e instanceof Error ? e.message : "Failed to open this file as a PDF.";
        if (!cancelled) setError(msg);
      }
    })();

    return () => {
      cancelled = true;
      void localDoc?.destroy();
      setDoc(null);
    };
  }, [path, epoch, documentId, resetRotations]);

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
      <ZoomToolbar
        onExtract={doc ? () => setExtractOpen(true) : undefined}
        onSplit={doc ? () => setSplitOpen(true) : undefined}
        onMerge={doc ? () => setMergeOpen(true) : undefined}
        onInsertFromPdf={doc ? () => setInsertFromOpen(true) : undefined}
        onWatermark={doc ? () => setWatermarkOpen(true) : undefined}
        onBackground={doc ? () => setBackgroundOpen(true) : undefined}
        onHeaderFooter={doc ? () => setHeaderFooterOpen(true) : undefined}
      />
      {doc ? <MarkupToolbar documentId={documentId} /> : null}
      <SearchBar />
      <FontFallbackBanner
        report={fontReport.report}
        dismissed={fontReport.dismissed}
        onDismiss={fontReport.dismiss}
      />
      <ExtractDialog
        open={extractOpen}
        pageCount={doc?.numPages ?? 0}
        onExtract={(pages) => void handleExtract(pages)}
        onClose={() => setExtractOpen(false)}
      />
      <SplitDialog
        open={splitOpen}
        pageCount={doc?.numPages ?? 0}
        onSplit={(mode) => void handleSplit(mode)}
        onClose={() => setSplitOpen(false)}
      />
      <MergeDialog
        open={mergeOpen}
        initialPaths={mergeSeedPaths}
        onMerge={(paths) => void handleMerge(paths)}
        onClose={() => setMergeOpen(false)}
      />
      <InsertFromDialog
        open={insertFromOpen}
        destPageCount={doc?.numPages ?? 0}
        onInsert={(args) => void handleInsertFromPdf(args)}
        onClose={() => setInsertFromOpen(false)}
      />
      <WatermarkDialog
        open={watermarkOpen}
        documentId={documentId}
        pageCount={doc?.numPages ?? 0}
        onClose={() => setWatermarkOpen(false)}
      />
      <BackgroundDialog
        open={backgroundOpen}
        documentId={documentId}
        pageCount={doc?.numPages ?? 0}
        onClose={() => setBackgroundOpen(false)}
      />
      <HeaderFooterDialog
        open={headerFooterOpen}
        documentId={documentId}
        pageCount={doc?.numPages ?? 0}
        onClose={() => setHeaderFooterOpen(false)}
      />
      <div className="flex flex-1 overflow-hidden">
        {showThumbnails && doc ? (
          // Key is per-panel, not bare `documentId`: this panel and
          // `AnnotationPanel` are siblings, so a shared key collides ("two
          // children with the same key") and React duplicates/omits one — which
          // showed up as ghost Pages columns. The prefix keeps the remount-on-
          // document-switch behaviour while staying unique among siblings.
          <ThumbnailPanel
            key={`thumbnails:${documentId}`}
            doc={doc}
            documentId={documentId}
            darkMode={darkMode}
            onJump={(page) => virtRef.current?.scrollToPage(page)}
          />
        ) : null}
        {showOutline && doc ? (
          <OutlinePanel
            doc={doc}
            onJump={(page) => virtRef.current?.scrollToPage(page)}
          />
        ) : null}
        {/* Not gated on `doc`: the panel reads annotations via the actor
            (documentId), so keeping it mounted across an edit reload (when `doc`
            briefly goes null) preserves its filter / search / composer state. The
            documentId key still remounts it on a real document switch. */}
        {showAnnotations ? (
          <AnnotationPanel
            key={`annotations:${documentId}`}
            documentId={documentId}
            epoch={epoch}
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
              epoch={epoch}
              initialPage={initialPageRef.current}
              initialScrollTop={initialScrollTopRef.current}
              zoom={zoom}
              fitMode={fitMode}
              darkMode={darkMode}
              onZoom={setZoom}
            />
          ) : (
            <div className="p-4 text-sm text-neutral-500">Opening…</div>
          )}
        </div>
      </div>
    </div>
  );
}
