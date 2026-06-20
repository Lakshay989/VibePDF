// SPEC: P3-ANN-004 (P3.C1b₂) — the polygon overlay's multi-click gesture: clicks
// add vertices, double-click / Enter finishes (persisting via addPolygon), Esc
// cancels, and a <3-vertex finish is ignored. IPC is mocked.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render } from "@testing-library/react";

vi.mock("@/ipc/polygons", () => ({
  addPolygon: vi.fn().mockResolvedValue({ canUndo: true, canRedo: false }),
}));

import { addPolygon } from "@/ipc/polygons";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useToolStore } from "@/state/tool-store";
import { PolygonLayer } from "@/view/polygon-layer";

const DOC = "doc-1";

// Letter (612×792), 1× scale, no rotation → screen (x,y) maps to PDF (x, 792−y).
const layer = () => (
  <PolygonLayer documentId={DOC} page={0} displayedWidth={612} displayedHeight={792} scale={1} rotation={0} />
);

const click = (svg: Element, x: number, y: number) =>
  fireEvent.pointerDown(svg, { clientX: x, clientY: y, pointerId: 1 });

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});
beforeEach(() => {
  useToolStore.setState({
    activeTool: "polygon",
    options: {
      color: "#112233",
      opacity: 1,
      strokeWidth: 2,
      fillColor: null,
      fontFamily: "Helvetica",
      fontSize: 14,
      bold: false,
      italic: false,
    },
  });
  useEditEpochStore.setState({ byDoc: {}, edited: {} });
});

describe("PolygonLayer", () => {
  it("commits a polygon: clicks add vertices, double-click finishes", () => {
    const { container } = render(layer());
    const svg = container.querySelector("svg") as Element;

    click(svg, 100, 100);
    click(svg, 200, 100);
    click(svg, 150, 180);
    fireEvent.doubleClick(svg);

    // screen (100,100)→PDF(100,692), (200,100)→(200,692), (150,180)→(150,612).
    expect(addPolygon).toHaveBeenCalledWith(
      DOC,
      0,
      true,
      [
        [100, 692],
        [200, 692],
        [150, 612],
      ],
      "#112233",
      null,
      1,
      2,
    );
    expect(useToolStore.getState().activeTool).toBeNull();
  });

  it("Enter also finishes", () => {
    const { container } = render(layer());
    const svg = container.querySelector("svg") as Element;
    click(svg, 100, 100);
    click(svg, 200, 100);
    click(svg, 150, 180);
    fireEvent.keyDown(window, { key: "Enter" });
    expect(addPolygon).toHaveBeenCalledTimes(1);
  });

  it("Escape cancels without persisting", () => {
    const { container } = render(layer());
    const svg = container.querySelector("svg") as Element;
    click(svg, 100, 100);
    click(svg, 200, 100);
    fireEvent.keyDown(window, { key: "Escape" });

    expect(addPolygon).not.toHaveBeenCalled();
    expect(useToolStore.getState().activeTool).toBeNull();
  });

  it("ignores a finish with fewer than 3 vertices", () => {
    const { container } = render(layer());
    const svg = container.querySelector("svg") as Element;
    click(svg, 100, 100);
    click(svg, 200, 100);
    fireEvent.doubleClick(svg);

    expect(addPolygon).not.toHaveBeenCalled();
    expect(useToolStore.getState().activeTool).toBeNull();
  });

  it("does nothing when the polygon tool is not active", () => {
    useToolStore.setState({ activeTool: null });
    const { container } = render(layer());
    const svg = container.querySelector("svg") as Element;
    click(svg, 100, 100);
    fireEvent.doubleClick(svg);
    expect(addPolygon).not.toHaveBeenCalled();
  });
});
