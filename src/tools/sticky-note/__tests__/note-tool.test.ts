// SPEC: P3-ANN-002 — the sticky-note placement reducer. A note is a
// click-to-place draft: a press produces an empty note at that PDF point and
// move/up carry it unchanged (no drag geometry).

import { describe, expect, it } from "vitest";

import {
  DEFAULT_NOTE_AUTHOR,
  NOTE_COLOR,
  noteDraftAt,
  stickyNoteTool,
} from "@/tools/sticky-note/note-tool";

const ctx = {
  documentId: "doc-1",
  options: {
    color: "#000",
    opacity: 1,
    strokeWidth: 2,
    fillColor: null,
    fontFamily: "Helvetica" as const,
    fontSize: 14,
    bold: false,
    italic: false,
  },
};

describe("noteDraftAt", () => {
  it("builds an empty note anchored at the point with the note colour", () => {
    const draft = noteDraftAt(2, 100, 200, "Ada");
    expect(draft).toEqual({
      type: "note",
      page: 2,
      point: { x: 100, y: 200 },
      content: "",
      author: "Ada",
      color: NOTE_COLOR,
      opacity: 1,
    });
  });
});

describe("stickyNoteTool", () => {
  it("places a note on pointer down using the default author", () => {
    const draft = stickyNoteTool.onPointerDown({ page: 0, x: 50, y: 60 }, ctx);
    expect(draft).not.toBeNull();
    if (!draft || draft.type !== "note") throw new Error("expected a note draft");
    expect(draft.point).toEqual({ x: 50, y: 60 });
    expect(draft.author).toBe(DEFAULT_NOTE_AUTHOR);
  });

  it("carries the draft unchanged through move and up (no drag)", () => {
    const draft = stickyNoteTool.onPointerDown({ page: 0, x: 50, y: 60 }, ctx);
    if (!draft) throw new Error("expected a draft");
    const moved = stickyNoteTool.onPointerMove(draft, { page: 0, x: 999, y: 999 }, ctx);
    expect(moved).toEqual(draft);
    const up = stickyNoteTool.onPointerUp(draft, { page: 0, x: 999, y: 999 }, ctx);
    expect(up).toEqual(draft);
  });
});
