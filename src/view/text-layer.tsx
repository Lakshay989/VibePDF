// SPEC: P3-ANN-001 (P3.B1a) — the PDF.js text layer.
//
// Renders selectable, transparent text spans over the page canvas (PDF.js's
// `TextLayer`), aligned to the same viewport the canvas uses. This makes text
// selectable, which is what the text-markup tools act on. Lives between the
// canvas and the annotation overlay in `PageSlot`; the overlay sits on top and
// is click-through when idle, so selection passes through to these spans.
//
// Styling is the minimal `.textLayer` port in `styles/globals.css`.

import { useEffect, useRef } from "react";
import { type PDFDocumentProxy, TextLayer } from "pdfjs-dist";

export interface PageTextLayerProps {
  doc: PDFDocumentProxy;
  /** 1-based page number. */
  pageNumber: number;
  /** CSS px per point (display scale, no devicePixelRatio). */
  scale: number;
  rotation: number;
}

export function PageTextLayer({ doc, pageNumber, scale, rotation }: PageTextLayerProps) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const container = ref.current;
    if (!container) return undefined;
    let cancelled = false;
    let layer: TextLayer | null = null;

    void (async () => {
      try {
        const page = await doc.getPage(pageNumber);
        const viewport = page.getViewport({ scale, rotation });
        const textContentSource = await page.getTextContent();
        if (cancelled) return;
        container.replaceChildren();
        // PDF.js positions spans relative to this custom property.
        container.style.setProperty("--scale-factor", String(scale));
        container.style.width = `${Math.floor(viewport.width)}px`;
        container.style.height = `${Math.floor(viewport.height)}px`;
        layer = new TextLayer({ textContentSource, container, viewport });
        await layer.render();
      } catch (err) {
        if (!cancelled) {
          console.warn("text layer render failed", pageNumber, err);
        }
      }
    })();

    return () => {
      cancelled = true;
      layer?.cancel();
    };
  }, [doc, pageNumber, scale, rotation]);

  return <div ref={ref} className="textLayer absolute left-0 top-0" />;
}
