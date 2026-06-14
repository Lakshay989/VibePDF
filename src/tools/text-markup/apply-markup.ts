// SPEC: P3-ANN-001 (P3.B1a) — apply a text-markup to the current selection.
//
// Text markup is *selection-driven*, not a pointer-drag gesture, so it does NOT
// go through the A1 `stepTool` lifecycle. The user selects text (the PDF.js text
// layer makes it selectable) and clicks a markup button; this reads
// `window.getSelection()`, groups its line rects by page, and adds one
// `MarkupAnnotation` per page to the store.
//
// The DOM-reading part can't run under jsdom (no layout), so the geometry→quads
// core (`buildMarkupDrafts`) is pure and unit-tested; `applyMarkupToSelection`
// is the thin DOM wrapper, exercised in-app.

import type { AnnotationDraft, MarkupSubtype, PageGeometry } from "@/tools/_framework";

import { type LocalRect, rectsToQuads } from "./quads";

export interface MarkupRequest {
  documentId: string;
  subtype: MarkupSubtype;
  color: string;
  opacity: number;
}

/** A page's geometry + the selection's page-local line rects on it. */
export interface PageGroup {
  geo: PageGeometry;
  rects: LocalRect[];
}

type MarkupDraft = Extract<AnnotationDraft, { type: "markup" }>;

/** Pure: per-page (geometry + local rects) → markup drafts (one per page). */
export function buildMarkupDrafts(groups: PageGroup[], req: MarkupRequest): MarkupDraft[] {
  const out: MarkupDraft[] = [];
  for (const group of groups) {
    const quads = rectsToQuads(group.rects, group.geo);
    if (quads.length === 0) continue;
    out.push({
      type: "markup",
      subtype: req.subtype,
      quads,
      page: group.geo.page,
      color: req.color,
      opacity: req.opacity,
    });
  }
  return out;
}

/** The page element under a viewport point (`[data-page]`), or null. */
function pageElementAt(x: number, y: number): HTMLElement | null {
  for (const el of document.elementsFromPoint(x, y)) {
    const page = (el as HTMLElement).closest?.("[data-page]");
    if (page) return page as HTMLElement;
  }
  return null;
}

/** Build a page's `PageGeometry` from its `data-*` attributes + live size. */
function geometryOf(pageEl: HTMLElement): PageGeometry | null {
  const pageNo = Number(pageEl.dataset.page);
  const pdfW = Number(pageEl.dataset.pdfW);
  const pdfH = Number(pageEl.dataset.pdfH);
  const rotation = Number(pageEl.dataset.rotation ?? "0");
  if (![pageNo, pdfW, pdfH].every(Number.isFinite) || pdfW <= 0 || pdfH <= 0) return null;
  const swap = (((rotation % 180) + 180) % 180) === 90;
  const displayedW = swap ? pdfH : pdfW;
  const cssWidth = pageEl.getBoundingClientRect().width;
  if (cssWidth <= 0) return null;
  // `data-page` is 1-based (the page number); coords/store use 0-based indices.
  return { page: pageNo - 1, width: pdfW, height: pdfH, scale: cssWidth / displayedW, rotation };
}

/** Group the live selection's line rects by the page element they fall on. */
function collectSelectionGroups(): PageGroup[] {
  const sel = window.getSelection();
  if (!sel || sel.isCollapsed || sel.rangeCount === 0) return [];

  const byPage = new Map<HTMLElement, LocalRect[]>();
  for (let i = 0; i < sel.rangeCount; i += 1) {
    for (const rect of Array.from(sel.getRangeAt(i).getClientRects())) {
      if (rect.width < 0.5 || rect.height < 0.5) continue;
      const pageEl = pageElementAt(rect.left + rect.width / 2, rect.top + rect.height / 2);
      if (!pageEl) continue;
      const pr = pageEl.getBoundingClientRect();
      const list = byPage.get(pageEl) ?? [];
      list.push({
        left: rect.left - pr.left,
        top: rect.top - pr.top,
        right: rect.right - pr.left,
        bottom: rect.bottom - pr.top,
      });
      byPage.set(pageEl, list);
    }
  }

  const groups: PageGroup[] = [];
  for (const [pageEl, rects] of byPage) {
    const geo = geometryOf(pageEl);
    if (geo) groups.push({ geo, rects });
  }
  return groups;
}

/**
 * Read the live text selection and add a markup annotation per page it covers.
 * Returns the number of annotations added (0 if nothing was selected).
 */
export function applyMarkupToSelection(
  req: MarkupRequest,
  add: (documentId: string, draft: AnnotationDraft) => unknown,
): number {
  const drafts = buildMarkupDrafts(collectSelectionGroups(), req);
  for (const draft of drafts) add(req.documentId, draft);
  if (drafts.length > 0) window.getSelection()?.removeAllRanges();
  return drafts.length;
}
