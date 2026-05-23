import { useEffect, useRef, useState } from "react";
import { readFile } from "@tauri-apps/plugin-fs";
import { renderPage } from "@/view/render-page";
import type { DocumentId } from "@/ipc/pdf";

interface Props {
  documentId: DocumentId;
  path: string;
}

// Phase 1 bootstrap: render page 1 of the open PDF to a single canvas.
// Virtual scrolling, zoom controls, search, thumbnails, and outline all
// land in follow-up commits within Phase 1. The point of this file is to
// prove that the PDF.js + Tauri + Vite worker pipeline holds end-to-end.
export function PdfViewer({ documentId, path }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const bytes = await readFile(path);
        if (cancelled || !canvasRef.current) return;
        await renderPage({
          data: bytes,
          pageNumber: 1,
          scale: 1.5,
          canvas: canvasRef.current,
        });
      } catch (e) {
        // SPEC: P1-VIEW-002 — invalid file → user-visible message, no crash.
        const msg =
          e instanceof Error ? e.message : "Failed to open this file as a PDF.";
        if (!cancelled) setError(msg);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [documentId, path]);

  return (
    <div className="h-full overflow-auto bg-neutral-100 p-4 dark:bg-neutral-900">
      {error ? (
        <div className="mx-auto max-w-lg rounded border border-red-300 bg-red-50 p-3 text-sm text-red-900 dark:border-red-800 dark:bg-red-950 dark:text-red-200">
          This file does not appear to be a valid PDF.
          <div className="mt-1 text-xs opacity-70">{error}</div>
        </div>
      ) : (
        <canvas ref={canvasRef} className="mx-auto block shadow" />
      )}
    </div>
  );
}
