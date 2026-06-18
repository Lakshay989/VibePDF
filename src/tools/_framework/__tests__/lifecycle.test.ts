import { describe, expect, it } from "vitest";

import { exampleRectTool } from "../example-rect-tool";
import { IDLE, stepTool, type ToolSession } from "../lifecycle";
import type { ToolContext } from "../tool-contract";
import type { PagePoint } from "../types";

const ctx: ToolContext = {
  documentId: "doc-1",
  options: {
    color: "#ff0000",
    opacity: 0.5,
    strokeWidth: 3,
    fontFamily: "Helvetica",
    fontSize: 14,
    bold: false,
    italic: false,
  },
};
const pt = (x: number, y: number): PagePoint => ({ page: 0, x, y });

describe("tool lifecycle", () => {
  it("drives a rectangle from down → move → up and commits it", () => {
    let s: ToolSession = IDLE;
    let r = stepTool(exampleRectTool, s, { kind: "pointerDown", pt: pt(10, 20) }, ctx);
    expect(r.session.phase).toBe("drawing");
    expect(r.committed).toBeNull();
    s = r.session;

    r = stepTool(exampleRectTool, s, { kind: "pointerMove", pt: pt(50, 80) }, ctx);
    expect(r.session.phase).toBe("drawing");
    s = r.session;

    r = stepTool(exampleRectTool, s, { kind: "pointerUp", pt: pt(50, 80) }, ctx);
    expect(r.session).toEqual(IDLE);
    const c = r.committed;
    if (!c || !("rect" in c)) throw new Error("expected a rect draft");
    expect(c.type).toBe("rectangle");
    expect(c.rect).toEqual({ x0: 10, y0: 20, x1: 50, y1: 80 });
    // Style flows from the active tool's options.
    expect(c.color).toBe("#ff0000");
    expect(c.strokeWidth).toBe(3);
  });

  it("normalizes a rect dragged up-left (the anchor survives crossing itself)", () => {
    let s = stepTool(exampleRectTool, IDLE, { kind: "pointerDown", pt: pt(50, 80) }, ctx).session;
    s = stepTool(exampleRectTool, s, { kind: "pointerMove", pt: pt(10, 20) }, ctx).session;
    const r = stepTool(exampleRectTool, s, { kind: "pointerUp", pt: pt(10, 20) }, ctx);
    const c = r.committed;
    if (!c || !("rect" in c)) throw new Error("expected a rect draft");
    expect(c.rect).toEqual({ x0: 10, y0: 20, x1: 50, y1: 80 });
  });

  it("commits nothing for a zero-area click (no drag)", () => {
    const s = stepTool(exampleRectTool, IDLE, { kind: "pointerDown", pt: pt(10, 20) }, ctx).session;
    const r = stepTool(exampleRectTool, s, { kind: "pointerUp", pt: pt(10, 20) }, ctx);
    expect(r.committed).toBeNull();
    expect(r.session).toEqual(IDLE);
  });

  it("cancel drops the draft and returns to idle", () => {
    const s = stepTool(exampleRectTool, IDLE, { kind: "pointerDown", pt: pt(10, 20) }, ctx).session;
    const r = stepTool(exampleRectTool, s, { kind: "cancel" }, ctx);
    expect(r.session).toEqual(IDLE);
    expect(r.committed).toBeNull();
  });

  it("ignores move/up while idle, and a re-press while drawing", () => {
    expect(stepTool(exampleRectTool, IDLE, { kind: "pointerMove", pt: pt(1, 1) }, ctx).session).toEqual(IDLE);
    expect(stepTool(exampleRectTool, IDLE, { kind: "pointerUp", pt: pt(1, 1) }, ctx).session).toEqual(IDLE);

    const s = stepTool(exampleRectTool, IDLE, { kind: "pointerDown", pt: pt(10, 20) }, ctx).session;
    const r = stepTool(exampleRectTool, s, { kind: "pointerDown", pt: pt(99, 99) }, ctx);
    expect(r.session).toBe(s); // unchanged — the first draft is kept
    expect(r.committed).toBeNull();
  });
});
