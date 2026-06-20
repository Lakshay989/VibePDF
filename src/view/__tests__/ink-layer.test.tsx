// SPEC: P3-ANN-005 (P3.C2) — the ink overlay's drag gesture: pointer-down starts
// capture, moves accumulate samples, pointer-up smooths + persists via addInk. A
// tap (no movement) is ignored, and an inactive tool is a no-op. IPC is mocked.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render } from "@testing-library/react";

vi.mock("@/ipc/ink", () => ({
  addInk: vi.fn().mockResolvedValue({ canUndo: true, canRedo: false }),
}));

import { addInk } from "@/ipc/ink";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useToolStore } from "@/state/tool-store";
import { InkLayer } from "@/view/ink-layer";

const DOC = "doc-1";
const mockAddInk = vi.mocked(addInk);

// Letter (612×792), 1× scale, no rotation → screen (x,y) maps to PDF (x, 792−y).
const layer = () => (
  <InkLayer documentId={DOC} page={0} displayedWidth={612} displayedHeight={792} scale={1} rotation={0} />
);

// jsdom doesn't fully implement pointer capture; stub it so the gesture runs.
const stubCapture = (el: Element) => {
  Object.assign(el, {
    setPointerCapture: vi.fn(),
    hasPointerCapture: vi.fn(() => false),
    releasePointerCapture: vi.fn(),
  });
};

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});
beforeEach(() => {
  useToolStore.setState({
    activeTool: "ink",
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

describe("InkLayer", () => {
  it("commits a stroke: down → moves → up persists via addInk", () => {
    const { container } = render(layer());
    const svg = container.querySelector("svg") as Element;
    stubCapture(svg);

    fireEvent.pointerDown(svg, { clientX: 100, clientY: 100, pointerId: 1 });
    fireEvent.pointerMove(svg, { clientX: 140, clientY: 92, pointerId: 1 });
    fireEvent.pointerMove(svg, { clientX: 180, clientY: 100, pointerId: 1 });
    fireEvent.pointerUp(svg, { clientX: 180, clientY: 100, pointerId: 1 });

    expect(mockAddInk).toHaveBeenCalledTimes(1);
    const [doc, page, points, color, opacity, width] = mockAddInk.mock.calls[0];
    expect(doc).toBe(DOC);
    expect(page).toBe(0);
    // Smoothing preserves the origin: screen (100,100) → PDF (100, 692).
    expect(points[0][0]).toBeCloseTo(100);
    expect(points[0][1]).toBeCloseTo(692);
    expect(points.length).toBeGreaterThan(2);
    expect(color).toBe("#112233");
    expect(opacity).toBe(1);
    expect(width).toBe(2);
  });

  it("ignores a tap (down then up, no movement)", () => {
    const { container } = render(layer());
    const svg = container.querySelector("svg") as Element;
    stubCapture(svg);

    fireEvent.pointerDown(svg, { clientX: 100, clientY: 100, pointerId: 1 });
    fireEvent.pointerUp(svg, { clientX: 100, clientY: 100, pointerId: 1 });

    expect(mockAddInk).not.toHaveBeenCalled();
  });

  it("does nothing when the ink tool is not active", () => {
    useToolStore.setState({ activeTool: null });
    const { container } = render(layer());
    const svg = container.querySelector("svg") as Element;
    stubCapture(svg);

    fireEvent.pointerDown(svg, { clientX: 100, clientY: 100, pointerId: 1 });
    fireEvent.pointerMove(svg, { clientX: 150, clientY: 120, pointerId: 1 });
    fireEvent.pointerUp(svg, { clientX: 150, clientY: 120, pointerId: 1 });

    expect(mockAddInk).not.toHaveBeenCalled();
  });
});
