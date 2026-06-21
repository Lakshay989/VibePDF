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

import { useToolStore } from "@/state/tool-store";
import type { ToolId } from "@/tools/_framework/types";

export interface PageTextLayerProps {
  doc: PDFDocumentProxy;
  /** 1-based page number. */
  pageNumber: number;
  /** CSS px per point (display scale, no devicePixelRatio). */
  scale: number;
  rotation: number;
}

/**
 * Whether the active tool operates *on* the page text (so the text layer must
 * stay selectable). The markup tools act on a text selection, and idle/select
 * (`null`) lets the user select text to copy. Every other tool *draws* — over a
 * text run, a selectable layer would steal the pointer (the spans carry
 * `z-index:1`, painting above the overlay) and a drag would select text instead
 * of drawing, even escaping to a whole-document selection when it leaves the
 * page. Those tools get a non-interactive text layer.
 */
export function toolUsesTextSelection(tool: ToolId | null): boolean {
  return (
    tool === null ||
    tool === "highlight" ||
    tool === "underline" ||
    tool === "strikethrough" ||
    tool === "squiggly"
  );
}

export function PageTextLayer({ doc, pageNumber, scale, rotation }: PageTextLayerProps) {
  const ref = useRef<HTMLDivElement>(null);
  const activeTool = useToolStore((s) => s.activeTool);
  const interactive = toolUsesTextSelection(activeTool);

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
        // PDF.js v5 sizes the layer from `--scale-factor` (→ `--total-scale-factor`
        // in CSS); `render()` sets the width/height itself, so we don't.
        container.style.setProperty("--scale-factor", String(scale));
        layer = new TextLayer({ textContentSource, container, viewport });
        await layer.render();
        // PDF.js v5 sizes the layer with CSS `round()`, which WKWebView may not
        // support — pin an explicit px size so the layer exactly overlays the
        // canvas (mis-sizing throws off where the selectable spans land).
        container.style.width = `${Math.round(viewport.width)}px`;
        container.style.height = `${Math.round(viewport.height)}px`;
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

  // Toggle interactivity via className, never an inline `style` prop: the render
  // effect sets `width`/`height` imperatively on this same node, and a managed
  // `style` prop would clobber them on the next re-render.
  return (
    <div
      ref={ref}
      className={
        "textLayer absolute left-0 top-0" + (interactive ? "" : " pointer-events-none select-none")
      }
    />
  );
}
