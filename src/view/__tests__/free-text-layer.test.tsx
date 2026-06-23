// SPEC: P3-ANN-003 (P3.B3a) — the free-text overlay: drag a box → a positioned
// editor appears → typing + Add persists through the actor (canvas then renders
// the /AP). IPC is mocked — this asserts the overlay's wiring, not the Rust write
// path (covered by the Rust free_text / cos tests).

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";

vi.mock("@/ipc/freetext", () => ({
  addFreeText: vi.fn().mockResolvedValue({ canUndo: true, canRedo: false }),
  updateFreeText: vi.fn().mockResolvedValue({ canUndo: true, canRedo: false }),
  readFreeText: vi.fn().mockResolvedValue({
    rect: [100, 642, 300, 692],
    text: "Hi",
    fontFamily: "Times",
    fontSize: 18,
    color: "#cc1133",
    bold: true,
    italic: false,
    underline: true,
  }),
}));
vi.mock("@/ipc/annotations", () => ({ readAnnotations: vi.fn().mockResolvedValue([]) }));

import { readAnnotations } from "@/ipc/annotations";
import { addFreeText, readFreeText, updateFreeText } from "@/ipc/freetext";
import { useAnnotationEditStore } from "@/state/annotation-edit-store";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useToolStore } from "@/state/tool-store";
import { FreeTextLayer } from "@/view/free-text-layer";

const DOC = "doc-1";

// Letter (612×792), 1× scale, no rotation → PDF (x,y) maps to screen (x, 792−y).
const layer = () => (
  <FreeTextLayer
    documentId={DOC}
    page={0}
    displayedWidth={612}
    displayedHeight={792}
    scale={1}
    rotation={0}
  />
);

const drag = (root: Element, from: [number, number], to: [number, number]) => {
  fireEvent.pointerDown(root, { clientX: from[0], clientY: from[1], pointerId: 1 });
  fireEvent.pointerMove(root, { clientX: to[0], clientY: to[1], pointerId: 1 });
  fireEvent.pointerUp(root, { clientX: to[0], clientY: to[1], pointerId: 1 });
};

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});
beforeEach(() => {
  useToolStore.setState({
    activeTool: "free-text",
    options: {
      color: "#112233",
      opacity: 1,
      strokeWidth: 2,
      fillColor: null,
      fontFamily: "Times",
      fontSize: 18,
      bold: true,
      italic: false,
      underline: false,
    },
  });
  useEditEpochStore.setState({ byDoc: {}, edited: {} });
  useAnnotationEditStore.setState({ editing: null });
});

describe("FreeTextLayer", () => {
  it("opens an editor after a drag and persists on Add with the box + style", () => {
    const { container } = render(layer());
    const root = container.firstElementChild as Element;

    drag(root, [100, 100], [300, 150]);
    const textarea = container.querySelector('textarea[aria-label="Free-text annotation"]');
    expect(textarea).not.toBeNull();

    fireEvent.change(textarea as Element, { target: { value: "Hello" } });
    fireEvent.click(container.querySelector('button[aria-label="Add free text"]') as Element);

    // screen (100,100)→PDF(100,692), (300,150)→PDF(300,642); sorted.
    expect(addFreeText).toHaveBeenCalledWith(
      DOC,
      0,
      [100, 642, 300, 692],
      "Hello",
      "Times",
      18,
      "#112233",
      true,
      false,
      false,
    );
    // Editor closes and the tool drops to idle after committing.
    expect(container.querySelector("textarea")).toBeNull();
    expect(useToolStore.getState().activeTool).toBeNull();
  });

  it("expands a click (no real drag) to a default-size box", () => {
    const { container } = render(layer());
    const root = container.firstElementChild as Element;

    drag(root, [50, 50], [51, 51]); // a click
    const editor = container.querySelector('textarea[aria-label="Free-text annotation"]');
    expect(editor).not.toBeNull(); // still get a usable box
  });

  it("Cancel discards the box without an IPC call", () => {
    const { container } = render(layer());
    const root = container.firstElementChild as Element;

    drag(root, [100, 100], [300, 150]);
    fireEvent.change(
      container.querySelector("textarea") as Element,
      { target: { value: "throwaway" } },
    );
    fireEvent.click(container.querySelector("button") as Element); // first button = Cancel

    expect(addFreeText).not.toHaveBeenCalled();
    expect(container.querySelector("textarea")).toBeNull();
    expect(useToolStore.getState().activeTool).toBeNull();
  });

  it("opens pre-filled on an edit request and commits via updateFreeText", () => {
    useToolStore.setState({ activeTool: null }); // edit doesn't need the tool
    useAnnotationEditStore.getState().requestEdit({
      nm: "ft-1",
      page: 0,
      data: {
        rect: [100, 642, 300, 692],
        text: "Helo",
        fontFamily: "Times",
        fontSize: 18,
        color: "#cc1133",
        bold: true,
        italic: false,
        underline: false,
      },
    });
    const { container } = render(layer());

    const textarea = container.querySelector(
      'textarea[aria-label="Free-text annotation"]',
    ) as HTMLTextAreaElement;
    expect(textarea).not.toBeNull();
    expect(textarea.value).toBe("Helo");
    // The toolbar options were set to the box's style for a faithful preview.
    expect(useToolStore.getState().options.fontFamily).toBe("Times");
    expect(useToolStore.getState().options.color).toBe("#cc1133");

    fireEvent.change(textarea, { target: { value: "Hello" } });
    fireEvent.click(container.querySelector('button[aria-label="Add free text"]') as Element);

    expect(updateFreeText).toHaveBeenCalledWith("doc-1", "ft-1", "Hello", "Times", 18, "#cc1133", true, false, false);
    expect(addFreeText).not.toHaveBeenCalled();
  });

  it("does nothing when the free-text tool is not active", () => {
    useToolStore.setState({ activeTool: null });
    const { container } = render(layer());
    const root = container.firstElementChild as Element;
    drag(root, [100, 100], [300, 150]);
    expect(container.querySelector("textarea")).toBeNull();
  });

  // SPEC: P3-ANN-013 (P3.B3b) — double-click a committed box → re-edit.
  it("double-clicking a committed free-text box opens it for re-edit", async () => {
    useToolStore.setState({ activeTool: null }); // idle: hit-zones are active
    vi.mocked(readAnnotations).mockResolvedValueOnce([
      { id: "ft-9", page: 0, kind: "freetext", rect: [100, 642, 300, 692], contents: "Hi", author: "", modified: null, inReplyTo: null },
    ]);
    const { container } = render(layer());

    const zone = await waitFor(() => {
      const z = container.querySelector('[title="Double-click to edit"]');
      expect(z).not.toBeNull();
      return z as Element;
    });
    fireEvent.doubleClick(zone);

    expect(readFreeText).toHaveBeenCalledWith(DOC, "ft-9");
    // The editor opens pre-filled with the box's text (the layer consumes the
    // edit request immediately, so observe the editor, not the transient store).
    const textarea = await waitFor(() => {
      const t = container.querySelector('textarea[aria-label="Free-text annotation"]') as HTMLTextAreaElement;
      expect(t).not.toBeNull();
      return t;
    });
    expect(textarea.value).toBe("Hi");
  });
});
