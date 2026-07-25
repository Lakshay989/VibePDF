// SPEC: P4-EDIT-003 (P4.B2) — the add-text overlay: drag a box → a positioned
// editor appears → typing + Add persists through the actor (addTextBox, content
// stream — not an annotation). IPC mocked; this asserts the overlay's wiring.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";

vi.mock("@/ipc/text-box", () => ({
  addTextBox: vi.fn().mockResolvedValue({ canUndo: true, canRedo: false }),
  updateTextBox: vi.fn().mockResolvedValue({ canUndo: true, canRedo: false }),
  readTextBoxes: vi.fn().mockResolvedValue([]),
}));

import { addTextBox, readTextBoxes, updateTextBox } from "@/ipc/text-box";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useToolStore } from "@/state/tool-store";
import { TextBoxLayer } from "@/view/text-box-layer";

const DOC = "doc-1";
const mockAdd = vi.mocked(addTextBox);
const mockUpdate = vi.mocked(updateTextBox);
const mockRead = vi.mocked(readTextBoxes);

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

  it("is a pass-through layer (no editor) when inactive with no boxes", () => {
    useToolStore.setState({ activeTool: null });
    const { container } = render(layer());
    // SPEC: P4-EDIT-003b — idle now renders a pass-through overlay (for re-edit
    // hit-zones), but with no boxes it shows no editor and no hit-zones.
    expect(container.querySelector("textarea")).toBeNull();
    expect(container.querySelector('[title="Double-click to edit this text"]')).toBeNull();
  });

  it("re-edits an existing box via double-click → updateTextBox (rect preserved server-side)", async () => {
    useToolStore.setState({ activeTool: null });
    mockRead.mockResolvedValueOnce([
      {
        id: "box-7",
        rect: [100, 642, 300, 692],
        text: "old text",
        fontFamily: "Times",
        fontSize: 18,
        color: "#112233",
        bold: true,
        italic: false,
        underline: false,
      },
    ]);
    const { container } = render(layer());

    const zone = await waitFor(() => {
      const z = container.querySelector('[title="Double-click to edit this text"]');
      expect(z).not.toBeNull();
      return z as Element;
    });
    fireEvent.doubleClick(zone);

    const textarea = container.querySelector(
      'textarea[aria-label="Add text to the page"]',
    ) as HTMLTextAreaElement;
    expect(textarea).not.toBeNull();
    expect(textarea.value).toBe("old text"); // pre-filled from the box

    fireEvent.change(textarea, { target: { value: "new text" } });
    fireEvent.click(container.querySelector('button[aria-label="Save text edit"]') as Element);

    expect(mockUpdate).toHaveBeenCalledTimes(1);
    const [doc, page, boxId, newText] = mockUpdate.mock.calls[0];
    expect(doc).toBe(DOC);
    expect(page).toBe(0);
    expect(boxId).toBe("box-7");
    expect(newText).toBe("new text");
    // A re-edit must never fall through to the "add a new box" path.
    expect(mockAdd).not.toHaveBeenCalled();
  });
});
