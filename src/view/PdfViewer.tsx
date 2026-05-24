import { useEffect, useRef, useState } from "react";
import { readFile } from "@tauri-apps/plugin-fs";
import type { PDFDocumentProxy } from "pdfjs-dist";

import { loadDocument } from "@/view/render-page";
import {
  PageVirtualizer,
  type PageVirtualizerHandle,
} from "@/view/PageVirtualizer";
import { isInputFocused, keyToIntent } from "@/view/keyboard-nav";
import type { DocumentId } from "@/ipc/pdf";

interface Props {
  documentId: DocumentId;
  path: string;
}

// SPEC: P1-VIEW-001, P1-VIEW-005, NFR-PERF-003.
//
// PdfViewer owns the document lifecycle: load once, hand the proxy to
// the virtualizer for the lifetime of the open tab, destroy on unmount.
// Per-page rendering and scrolling live in PageVirtualizer; this
// component wires the keyboard listener that drives it.
export function PdfViewer({ documentId, path }: Props) {
  const [doc, setDoc] = useState<PDFDocumentProxy | null>(null);
  const [error, setError] = useState<string | null>(null);
  const virtRef = useRef<PageVirtualizerHandle>(null);

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

  // SPEC: P1-VIEW-005 (P1.C3) — PageUp/Down, Home/End, Arrow keys.
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      const intent = keyToIntent(
        {
          key: e.key,
          shiftKey: e.shiftKey,
          ctrlKey: e.ctrlKey,
          metaKey: e.metaKey,
          altKey: e.altKey,
        },
        { inputFocused: isInputFocused(document.activeElement) },
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
  }, [doc]);

  return (
    <div className="h-full">
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
        />
      ) : (
        <div className="p-4 text-sm text-neutral-500">Opening…</div>
      )}
    </div>
  );
}
