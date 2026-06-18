// SPEC: P3-ANN-004 — the drag-to-size shape reducers (rectangle, ellipse): the
// anchor stays put while dragging, the rect normalizes on commit, and a zero-
// area click is discarded.

import { describe, expect, it } from "vitest";

import type { ToolContext } from "@/tools/_framework";
import { ellipseTool, rectangleTool } from "@/tools/shapes/shape-tools";

const ctx: ToolContext = {
  documentId: "doc-1",
  options: {
    color: "#ff0000",
    opacity: 1,
    strokeWidth: 2,
    fillColor: null,
    fontFamily: "Helvetica",
    fontSize: 14,
    bold: false,
    italic: false,
  },
};

describe.each([
  ["rectangle", rectangleTool],
  ["ellipse", ellipseTool],
] as const)("%s tool", (kind, tool) => {
  it("anchors on down and tracks the moving corner", () => {
    const down = tool.onPointerDown({ page: 0, x: 50, y: 60 }, ctx);
    expect(down).not.toBeNull();
    if (!down || (down.type !== "rectangle" && down.type !== "ellipse")) throw new Error("shape draft");
    expect(down.type).toBe(kind);
    expect(down.rect).toEqual({ x0: 50, y0: 60, x1: 50, y1: 60 });

    const moved = tool.onPointerMove(down, { page: 0, x: 200, y: 10 }, ctx);
    if (moved.type !== "rectangle" && moved.type !== "ellipse") throw new Error("shape draft");
    expect(moved.rect).toEqual({ x0: 50, y0: 60, x1: 200, y1: 10 });
  });

  it("normalizes on commit (drag up-left to down-right)", () => {
    const down = tool.onPointerDown({ page: 0, x: 200, y: 10 }, ctx);
    if (!down) throw new Error("draft");
    const up = tool.onPointerUp(down, { page: 0, x: 50, y: 60 }, ctx);
    if (!up || (up.type !== "rectangle" && up.type !== "ellipse")) throw new Error("shape draft");
    expect(up.rect).toEqual({ x0: 50, y0: 10, x1: 200, y1: 60 });
  });

  it("discards a zero-area click", () => {
    const down = tool.onPointerDown({ page: 0, x: 50, y: 60 }, ctx);
    if (!down) throw new Error("draft");
    expect(tool.onPointerUp(down, { page: 0, x: 50, y: 60 }, ctx)).toBeNull();
  });
});
