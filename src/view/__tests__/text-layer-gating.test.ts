// SPEC: P3-ANN-005 (P3.C2 fix) — the text layer must go non-interactive while a
// DRAWING tool is active, or a stroke that crosses text selects text (and can
// escape to a whole-document selection) instead of drawing. Only the markup
// tools and idle/select operate on the text itself.

import { describe, expect, it } from "vitest";

import { toolUsesTextSelection } from "@/view/text-layer";

describe("toolUsesTextSelection", () => {
  it("keeps text selectable for idle/select and the markup tools", () => {
    expect(toolUsesTextSelection(null)).toBe(true);
    expect(toolUsesTextSelection("highlight")).toBe(true);
    expect(toolUsesTextSelection("underline")).toBe(true);
    expect(toolUsesTextSelection("strikethrough")).toBe(true);
    expect(toolUsesTextSelection("squiggly")).toBe(true);
  });

  it("suppresses the text layer for every drawing tool", () => {
    for (const tool of [
      "ink",
      "rectangle",
      "ellipse",
      "line",
      "arrow",
      "polygon",
      "sticky-note",
      "free-text",
      "stamp",
    ] as const) {
      expect(toolUsesTextSelection(tool)).toBe(false);
    }
  });
});
