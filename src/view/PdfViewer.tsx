import { useEffect, useState } from "react";
import { readFile } from "@tauri-apps/plugin-fs";
import type { PDFDocumentProxy } from "pdfjs-dist";

import { loadDocument } from "@/view/render-page";
import { PageVirtualizer } from "@/view/PageVirtualizer";
import type { DocumentId } from "@/ipc/pdf";

interface Props {
  documentId: DocumentId;
  path: string;
}

// SPEC: P1-VIEW-001, P1-VIEW-005, NFR-PERF-003.
//
// PdfViewer now owns the document lifecycle: load once, hand the proxy
// to the virtualizer for the lifetime of the open tab, destroy on
// unmount. All per-page rendering is the virtualizer's responsibility.
export function PdfViewer({ documentId, path }: Props) {
  const [doc, setDoc] = useState<PDFDocumentProxy | null>(null);
  const [error, setError] = useState<string | null>(null);

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
        // SPEC: P1-VIEW-002 — invalid file → user-visible message, no crash.
        const msg =
          e instanceof Error ? e.message : "Failed to open this file as a PDF.";
        if (!cancelled) setError(msg);
      }
    })();
    return () => {
      cancelled = true;
      // Destroy is async but we don't await — React's cleanup is sync.
      // The promise is fire-and-forget; PDF.js handles late cleanup.
      void loaded?.destroy();
      setDoc(null);
    };
  }, [path]);

  return (
    <div className="h-full overflow-auto bg-neutral-100 dark:bg-neutral-900">
      {error ? (
        <div className="mx-auto mt-8 max-w-lg rounded border border-red-300 bg-red-50 p-3 text-sm text-red-900 dark:border-red-800 dark:bg-red-950 dark:text-red-200">
          This file does not appear to be a valid PDF.
          <div className="mt-1 text-xs opacity-70">{error}</div>
        </div>
      ) : doc ? (
        <PageVirtualizer doc={doc} documentId={documentId} />
      ) : (
        <div className="p-4 text-sm text-neutral-500">Opening…</div>
      )}
    </div>
  );
}
