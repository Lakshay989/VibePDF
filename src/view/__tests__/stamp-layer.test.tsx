// SPEC: P3-ANN-006 (P3.C3a) — the stamp overlay: with the stamp tool active and
// a stamp armed, a click drops it (centred, default size) via addStamp. Inactive
// tool or no armed stamp = no-op. IPC is mocked.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render } from "@testing-library/react";

vi.mock("@/ipc/stamps", () => ({
  addStamp: vi.fn().mockResolvedValue({ canUndo: true, canRedo: false }),
  addImageStamp: vi.fn().mockResolvedValue({ canUndo: true, canRedo: false }),
}));

import { addImageStamp, addStamp } from "@/ipc/stamps";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useStampStore } from "@/state/stamp-store";
import { useToolStore } from "@/state/tool-store";
import { StampLayer } from "@/view/stamp-layer";

const DOC = "doc-1";
const mockAddStamp = vi.mocked(addStamp);
const mockAddImageStamp = vi.mocked(addImageStamp);

// Letter (612×792), 1× scale → screen (x,y) maps to PDF (x, 792−y).
const layer = () => (
  <StampLayer documentId={DOC} page={0} displayedWidth={612} displayedHeight={792} scale={1} rotation={0} />
);

const APPROVED = { kind: "text", name: "Approved", label: "APPROVED", color: "#1e8449" } as const;

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});
beforeEach(() => {
  useToolStore.setState({
    activeTool: "stamp",
    options: {
      color: "#000000",
      opacity: 1,
      strokeWidth: 2,
      fillColor: null,
      fontFamily: "Helvetica",
      fontSize: 14,
      bold: false,
      italic: false,
    },
  });
  useStampStore.setState({ armed: APPROVED });
  useEditEpochStore.setState({ byDoc: {}, edited: {} });
});

describe("StampLayer", () => {
  it("drops the armed stamp on click, centred at the point", () => {
    const { container } = render(layer());
    const div = container.querySelector("div") as Element;
    fireEvent.pointerDown(div, { clientX: 300, clientY: 400, pointerId: 1, button: 0 });

    // screen (300,400) → PDF (300, 392); default 150×46 box centred + clamped.
    expect(mockAddStamp).toHaveBeenCalledWith(
      DOC,
      0,
      [225, 369, 375, 415],
      "APPROVED",
      "Approved",
      "#1e8449",
      1,
    );
  });

  // SPEC: P3-ANN-006 (P3.C3b) — an armed image stamp routes to addImageStamp with
  // the click point (the backend derives the aspect-correct rect).
  it("drops an armed image stamp at the click point", () => {
    useStampStore.setState({
      armed: { kind: "image", name: "sig.png", imagePath: "/tmp/sig.png" },
    });
    const { container } = render(layer());
    fireEvent.pointerDown(container.querySelector("div") as Element, {
      clientX: 300,
      clientY: 400,
      pointerId: 1,
      button: 0,
    });
    // screen (300,400) → PDF (300, 392); height 64, no label.
    expect(mockAddImageStamp).toHaveBeenCalledWith(DOC, 0, 300, 392, 64, "/tmp/sig.png", null, 1);
    expect(mockAddStamp).not.toHaveBeenCalled();
  });

  it("does nothing when no stamp is armed", () => {
    useStampStore.setState({ armed: null });
    const { container } = render(layer());
    fireEvent.pointerDown(container.querySelector("div") as Element, {
      clientX: 300,
      clientY: 400,
      pointerId: 1,
      button: 0,
    });
    expect(mockAddStamp).not.toHaveBeenCalled();
  });

  it("does nothing when the stamp tool is not active", () => {
    useToolStore.setState({ activeTool: null });
    const { container } = render(layer());
    fireEvent.pointerDown(container.querySelector("div") as Element, {
      clientX: 300,
      clientY: 400,
      pointerId: 1,
      button: 0,
    });
    expect(mockAddStamp).not.toHaveBeenCalled();
  });
});
