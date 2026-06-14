// SPEC: infrastructure (P3.A1) — the contract every annotation tool implements.
//
// A tool is a small pure state machine over a pointer gesture: it turns a
// pointerdown → move → up sequence (in PDF page coordinates) into a draft
// annotation to commit. Concrete tools (highlight, rectangle, ink, …) plug in
// via this interface; `lifecycle.ts` drives them, and the render layer (P3.A2)
// draws the live draft + committed annotations.
//
// Pointer events, NOT HTML5 drag-and-drop — WKWebView doesn't deliver DnD drop
// events (see docs/04 "WebView quirks"); the DOM binding in A2 uses
// `setPointerCapture`. Keeping each tool a pure reducer over `PagePoint`s makes
// the gesture logic unit-testable without a DOM.

import type { AnnotationDraft, PagePoint, ToolId, ToolOptions } from "./types";

/** Read-only context a tool sees on each gesture event. */
export interface ToolContext {
  documentId: string;
  options: ToolOptions;
}

/**
 * A tool: pure reducers from a pointer gesture to a draft annotation.
 *
 * - `onPointerDown` begins a draft (or returns `null` to ignore the press).
 * - `onPointerMove` evolves the draft as the pointer moves.
 * - `onPointerUp` finishes: returns the draft to commit, or `null` to discard.
 *
 * Tools hold no internal state — the in-progress draft is threaded through by
 * the lifecycle, so the same tool instance serves every gesture.
 */
export interface AnnotationTool {
  readonly id: ToolId;
  /** CSS cursor to show while this tool is active. */
  readonly cursor?: string;
  onPointerDown(pt: PagePoint, ctx: ToolContext): AnnotationDraft | null;
  onPointerMove(draft: AnnotationDraft, pt: PagePoint, ctx: ToolContext): AnnotationDraft;
  onPointerUp(draft: AnnotationDraft, pt: PagePoint, ctx: ToolContext): AnnotationDraft | null;
}
