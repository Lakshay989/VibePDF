// SPEC: P1-VIEW-005 (P1.C3) — keyboard navigation.
//
// The intent-mapping is a pure function so it can be unit-tested
// without a DOM. The PdfViewer is responsible for translating an
// intent into an imperative call on the virtualizer ref.

export type ScrollIntent =
  | { kind: "page-target"; page: "first" | "last" }
  | { kind: "page-delta"; delta: number } // +1 → next page, -1 → prev
  | { kind: "line-delta"; delta: number }; // px to scroll

export interface KeyEventLike {
  key: string;
  shiftKey?: boolean;
  ctrlKey?: boolean;
  metaKey?: boolean;
  altKey?: boolean;
}

export interface KeyContext {
  /** True iff a text input / textarea / contenteditable currently has focus. */
  inputFocused: boolean;
}

const LINE_PX = 40;

/**
 * Map a keyboard event to a navigation intent, or null if the key
 * has no mapping (or is being absorbed by an input).
 */
export function keyToIntent(
  e: KeyEventLike,
  ctx: KeyContext,
): ScrollIntent | null {
  // Never steal modifier combos; they belong to app-level shortcuts.
  if (e.ctrlKey || e.metaKey || e.altKey) return null;

  switch (e.key) {
    case "PageDown":
      return { kind: "page-delta", delta: +1 };
    case "PageUp":
      return { kind: "page-delta", delta: -1 };
    case "Home":
      return { kind: "page-target", page: "first" };
    case "End":
      return { kind: "page-target", page: "last" };
    case "ArrowDown":
      if (ctx.inputFocused) return null;
      return { kind: "line-delta", delta: +LINE_PX };
    case "ArrowUp":
      if (ctx.inputFocused) return null;
      return { kind: "line-delta", delta: -LINE_PX };
    default:
      return null;
  }
}

/** Heuristic: does the currently-focused element absorb arrow keys? */
export function isInputFocused(activeElement: Element | null): boolean {
  if (!activeElement) return false;
  const tag = activeElement.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return true;
  return (activeElement as HTMLElement).isContentEditable === true;
}
