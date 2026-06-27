// SPEC: P4-EDIT-003 (P4.B2) — the add-text overlay: drag a box → a positioned
// editor appears → typing + Add persists through the actor (addTextBox, content
// stream — not an annotation). IPC mocked; this asserts the overlay's wiring.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render } from "@testing-library/react";

vi.mock("@/ipc/text-box", () => ({
  addTextBox: vi.fn().mockResolvedValue({ canUndo: true, canRedo: false }),
}));

import { addTextBox } from "@/ipc/text-box";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useToolStore } from "@/state/tool-store";
import { TextBoxLayer } from "@/view/text-box-layer";

const DOC = "doc-1";
const mockAdd = vi.mocked(addTextBox);

// Letter (612×792), 1× scale, no rotation → PDF (x,y) maps to screen (x, 792−y).
const layer = () => (
  <TextBoxLayer documentId={DOC} page={0} displayedWidth={612} displayedHeight={792} scale={1} rotation={0} />
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
    activeTool: "add-text",
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
});

describe("TextBoxLayer", () => {
  it("opens an editor after a drag and adds via addTextBox with the box + style", () => {
    const { container } = render(layer());
    const root = container.firstElementChild as Element;

    drag(root, [100, 100], [300, 150]);
    const textarea = container.querySelector('textarea[aria-label="Add text to the page"]');
    expect(textarea).not.toBeNull();

    fireEvent.change(textarea as Element, { target: { value: "Hello page" } });
    fireEvent.click(container.querySelector('button[aria-label="Add text to page"]') as Element);

    expect(mockAdd).toHaveBeenCalledTimes(1);
    const [doc, page, , text, family, size, , bold] = mockAdd.mock.calls[0];
    expect(doc).toBe(DOC);
    expect(page).toBe(0);
    expect(text).toBe("Hello page");
    expect(family).toBe("Times");
    expect(size).toBe(18);
    expect(bold).toBe(true);
  });

  it("does not persist an empty box", () => {
    const { container } = render(layer());
    const root = container.firstElementChild as Element;
    drag(root, [100, 100], [300, 150]);
    // Commit without typing.
    fireEvent.click(container.querySelector('button[aria-label="Add text to page"]') as Element);
    expect(mockAdd).not.toHaveBeenCalled();
  });

  it("renders nothing when the add-text tool is inactive", () => {
    useToolStore.setState({ activeTool: null });
    const { container } = render(layer());
    expect(container.firstChild).toBeNull();
  });
});
