// SPEC: P3-ANN-007 (P3.C4a) — the measure overlay: distance auto-finishes on the
// 2nd click; area finishes on double-click; a calibration draw hands its
// point-length to the store (not persisted). IPC mocked.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render } from "@testing-library/react";

vi.mock("@/ipc/measure", () => ({
  addMeasure: vi.fn().mockResolvedValue({ canUndo: true, canRedo: false }),
}));

import { addMeasure } from "@/ipc/measure";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useMeasureStore } from "@/state/measure-store";
import { useToolStore } from "@/state/tool-store";
import { MeasureLayer } from "@/view/measure-layer";

const DOC = "doc-1";
const mockAddMeasure = vi.mocked(addMeasure);

// Letter (612×792), 1× scale → screen (x,y) maps to PDF (x, 792−y).
const layer = () => (
  <MeasureLayer documentId={DOC} page={0} displayedWidth={612} displayedHeight={792} scale={1} rotation={0} />
);

const click = (svg: Element, x: number, y: number) =>
  fireEvent.pointerDown(svg, { clientX: x, clientY: y, pointerId: 1, button: 0 });

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});
beforeEach(() => {
  useToolStore.setState({
    activeTool: "measure",
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
  useMeasureStore.setState({ kind: "distance", calibration: {}, calibrating: false, pendingRefPoints: null });
  useEditEpochStore.setState({ byDoc: {}, edited: {} });
});

describe("MeasureLayer", () => {
  it("distance auto-finishes on the 2nd click, with the value label", () => {
    const { container } = render(layer());
    const svg = container.querySelector("svg") as Element;
    click(svg, 100, 100);
    click(svg, 300, 100);

    // PDF (100,692)→(300,692): 200 pt, uncalibrated → "200 pt".
    expect(mockAddMeasure).toHaveBeenCalledWith(
      DOC,
      0,
      "distance",
      [
        [100, 692],
        [300, 692],
      ],
      "#112233",
      "200 pt",
      1,
      2,
    );
  });

  it("area finishes on double-click", () => {
    useMeasureStore.setState({ kind: "area" });
    const { container } = render(layer());
    const svg = container.querySelector("svg") as Element;
    click(svg, 100, 100);
    click(svg, 300, 100);
    click(svg, 200, 300);
    fireEvent.doubleClick(svg);

    expect(mockAddMeasure).toHaveBeenCalledTimes(1);
    const [doc, page, kind, points, , label] = mockAddMeasure.mock.calls[0];
    expect(doc).toBe(DOC);
    expect(page).toBe(0);
    expect(kind).toBe("area");
    expect(points).toHaveLength(3);
    expect(label).toMatch(/pt²$/);
  });

  it("a calibration draw stashes the reference length, not a measurement", () => {
    useMeasureStore.setState({ calibrating: true });
    const { container } = render(layer());
    const svg = container.querySelector("svg") as Element;
    click(svg, 100, 100);
    click(svg, 300, 100);

    expect(mockAddMeasure).not.toHaveBeenCalled();
    expect(useMeasureStore.getState().pendingRefPoints).toBeCloseTo(200);
    expect(useMeasureStore.getState().calibrating).toBe(false);
  });

  it("does nothing when the measure tool is not active", () => {
    useToolStore.setState({ activeTool: null });
    const { container } = render(layer());
    const svg = container.querySelector("svg") as Element;
    click(svg, 100, 100);
    click(svg, 300, 100);
    expect(mockAddMeasure).not.toHaveBeenCalled();
  });
});
