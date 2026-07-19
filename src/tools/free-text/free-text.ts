// SPEC: P3-ANN-003 (P3.B3a) — pure helpers for the free-text tool.
//
// The free-text box is a drag gesture (like the shape tools), but the committed
// box is canvas-rendered from a generated `/AP` (like markup) — only the
// transient editor lives in the overlay. These DOM-free helpers (font catalog,
// CSS mapping, screen-rect math) keep the gesture logic unit-testable.

import type { FontFamily } from "@/tools/_framework";

/** Families offered in the toolbar — the base-14 set we can draw without embedding. */
export const FONT_FAMILIES: readonly FontFamily[] = ["Helvetica", "Times", "Courier"];

/** A CSS font stack approximating each base-14 family for the editor preview. */
export function cssFontFamily(family: FontFamily): string {
  switch (family) {
    case "Times":
      return "'Times New Roman', Times, serif";
    case "Courier":
      return "'Courier New', Courier, monospace";
    case "Helvetica":
    default:
      return "Helvetica, Arial, sans-serif";
  }
}

// Screen-rect drag helpers (`ScreenRect`, `normalizeScreenRect`,
// `withDefaultSize`, `MIN_DRAG_PX`, `DEFAULT_BOX_PX`) moved to
// `@/tools/_framework` — they're framework-level, used by every rect-drawing
// layer, not free-text-specific (FABLE_REVIEW §3.15).
