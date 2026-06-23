// SPEC: P3-ANN-004 — the drag line + arrow reducers: press = start, release =
// end; the arrow tool sets `arrow`; a zero-length click is discarded.

import { describe, expect, it } from "vitest";

import type { ToolContext } from "@/tools/_framework";
import { arrowTool, lineTool } from "@/tools/shapes/line-tools";

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
    underline: false,
  },
};

describe("line / arrow tools", () => {
  it("starts at the press point and tracks the end", () => {
    const down = lineTool.onPointerDown({ page: 0, x: 50, y: 60 }, ctx);
    if (!down || down.type !== "line") throw new Error("line draft");
    expect(down.start).toEqual({ x: 50, y: 60 });
    expect(down.end).toEqual({ x: 50, y: 60 });
    expect(down.arrow).toBe(false);

    const moved = lineTool.onPointerMove(down, { page: 0, x: 200, y: 10 }, ctx);
    if (moved.type !== "line") throw new Error("line draft");
    expect(moved.end).toEqual({ x: 200, y: 10 });
  });

  it("the arrow tool sets the arrow flag", () => {
    const down = arrowTool.onPointerDown({ page: 0, x: 0, y: 0 }, ctx);
    if (!down || down.type !== "line") throw new Error("line draft");
    expect(down.arrow).toBe(true);
  });

  it("commits a real drag and discards a zero-length click", () => {
    const down = lineTool.onPointerDown({ page: 0, x: 50, y: 60 }, ctx);
    if (!down) throw new Error("expected a draft");
    const up = lineTool.onPointerUp(down, { page: 0, x: 200, y: 10 }, ctx);
    if (!up || up.type !== "line") throw new Error("line draft");
    expect(up.end).toEqual({ x: 200, y: 10 });

    expect(lineTool.onPointerUp(down, { page: 0, x: 50, y: 60 }, ctx)).toBeNull();
  });
});
