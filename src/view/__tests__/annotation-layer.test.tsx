import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { cleanup, fireEvent, render } from "@testing-library/react";

import { useAnnotationStore } from "@/state/annotation-store";
import { useToolStore } from "@/state/tool-store";
import { clearTools, registerTool, type Annotation } from "@/tools/_framework";
import { exampleRectTool } from "@/tools/_framework/example-rect-tool";
import { AnnotationLayer } from "@/view/annotation-layer";

const DOC = "doc-1";

// Letter (612×792), 1× scale, no rotation → PDF (x,y) maps to (x, 792−y).
const layer = () => (
  <AnnotationLayer
    documentId={DOC}
    page={0}
    displayedWidth={612}
    displayedHeight={792}
    scale={1}
    rotation={0}
  />
);

const rectAnn = (): Annotation => ({
  id: "a1",
  type: "rectangle",
  page: 0,
  rect: { x0: 10, y0: 20, x1: 110, y1: 120 },
  color: "#ff0000",
  opacity: 1,
  strokeWidth: 2,
  createdAt: 1,
});

afterEach(() => {
  cleanup();
  clearTools();
});
beforeEach(() => {
  useAnnotationStore.setState({ byDoc: {}, draft: null, selectedId: null });
  useToolStore.setState({ activeTool: null });
});

describe("AnnotationLayer", () => {
  it("draws a committed rectangle at the mapped screen position", () => {
    useAnnotationStore.setState({ byDoc: { [DOC]: [rectAnn()] }, draft: null, selectedId: null });
    const { container } = render(layer());

    const rect = container.querySelector('rect[data-ann-id="a1"]');
    expect(rect).not.toBeNull();
    // PDF (10,20)-(110,120) → screen x=10, y=792−120=672, 100×100.
    expect(rect?.getAttribute("x")).toBe("10");
    expect(rect?.getAttribute("y")).toBe("672");
    expect(rect?.getAttribute("width")).toBe("100");
    expect(rect?.getAttribute("height")).toBe("100");
    expect(rect?.getAttribute("stroke")).toBe("#ff0000");
  });

  it("draws the live draft (dashed)", () => {
    useAnnotationStore.setState({
      byDoc: {},
      draft: {
        type: "rectangle",
        page: 0,
        rect: { x0: 0, y0: 0, x1: 50, y1: 50 },
        color: "#0000ff",
        opacity: 1,
        strokeWidth: 1,
      },
      selectedId: null,
    });
    const { container } = render(layer());

    const draft = container.querySelector('rect[data-ann-id="__draft__"]');
    expect(draft).not.toBeNull();
    expect(draft?.getAttribute("stroke-dasharray")).toBe("4 4");
  });

  it("commits an annotation from pointer down → move → up with the rect tool active", () => {
    registerTool(exampleRectTool);
    useToolStore.setState({ activeTool: "rectangle" });
    const { container } = render(layer());
    const svg = container.querySelector("svg") as Element;

    fireEvent.pointerDown(svg, { clientX: 10, clientY: 20, pointerId: 1 });
    fireEvent.pointerMove(svg, { clientX: 110, clientY: 120, pointerId: 1 });
    fireEvent.pointerUp(svg, { clientX: 110, clientY: 120, pointerId: 1 });

    const list = useAnnotationStore.getState().byDoc[DOC];
    expect(list).toHaveLength(1);
    // down (10,20)→PDF(10,772), up (110,120)→PDF(110,672), normalized.
    expect(list?.[0]?.rect).toEqual({ x0: 10, y0: 672, x1: 110, y1: 772 });
    expect(useAnnotationStore.getState().draft).toBeNull();
  });

  it("selects an annotation on click when no tool is active", () => {
    useAnnotationStore.setState({ byDoc: { [DOC]: [rectAnn()] }, draft: null, selectedId: null });
    const { container } = render(layer());
    const rect = container.querySelector('rect[data-ann-id="a1"]') as Element;

    fireEvent.pointerDown(rect, { clientX: 15, clientY: 25, pointerId: 1 });
    expect(useAnnotationStore.getState().selectedId).toBe("a1");
  });
});
