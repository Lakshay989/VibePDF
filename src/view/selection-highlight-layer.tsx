// SPEC: P3-ANN-008 (P3.D1) — draws a dashed box over the annotation the user
// clicked in the sidebar, so "select" has visible feedback for every kind
// (markup / note / free-text), all of which are otherwise canvas- or icon-drawn.
// Purely visual (`pointer-events: none`); geometry from the read path's /Rect.

import { useAnnotationSelectionStore } from "@/state/annotation-selection-store";
import { type PageGeometry, pdfToScreen } from "@/tools/_framework";

export interface SelectionHighlightLayerProps {
  /** 0-based page index. */
  page: number;
  /** Displayed (rotation-swapped) page size in PDF points. */
  displayedWidth: number;
  displayedHeight: number;
  /** CSS px per point. */
  scale: number;
  /** Page display rotation in degrees. */
  rotation: number;
}

export function SelectionHighlightLayer({
  page,
  displayedWidth,
  displayedHeight,
  scale,
  rotation,
}: SelectionHighlightLayerProps) {
  const selected = useAnnotationSelectionStore((s) => s.selected);
  if (!selected || selected.page !== page) return null;

  const swapped = (((rotation % 180) + 180) % 180) === 90;
  const geo: PageGeometry = {
    page,
    width: swapped ? displayedHeight : displayedWidth,
    height: swapped ? displayedWidth : displayedHeight,
    scale,
    rotation,
  };

  const [x0, y0, x1, y1] = selected.rect;
  const a = pdfToScreen({ page, x: x0, y: y0 }, geo);
  const b = pdfToScreen({ page, x: x1, y: y1 }, geo);
  const left = Math.min(a.x, b.x);
  const top = Math.min(a.y, b.y);
  const width = Math.abs(b.x - a.x);
  const height = Math.abs(b.y - a.y);

  return (
    <div
      data-testid="annotation-selection"
      className="absolute rounded-sm border-2 border-dashed border-blue-500"
      style={{
        left: left - 2,
        top: top - 2,
        width: width + 4,
        height: height + 4,
        pointerEvents: "none",
        background: "rgba(37,99,235,0.08)",
      }}
    />
  );
}
