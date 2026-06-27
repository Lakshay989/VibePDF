// SPEC: P4-EDIT-005 (P4.C1) — the add-image overlay: with an image armed, drag a
// box → the image is embedded via addImage with that box; the tool disarms after.
// IPC mocked.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render } from "@testing-library/react";

vi.mock("@/ipc/image", () => ({
  addImage: vi.fn().mockResolvedValue({ canUndo: true, canRedo: false }),
}));

import { addImage } from "@/ipc/image";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useImageAddStore } from "@/state/image-add-store";
import { useToolStore } from "@/state/tool-store";
import { ImageAddLayer } from "@/view/image-add-layer";

const DOC = "doc-1";
const mockAdd = vi.mocked(addImage);

// Letter (612×792), 1× scale, no rotation → PDF (x,y) maps to screen (x, 792−y).
const layer = () => (
  <ImageAddLayer documentId={DOC} page={0} displayedWidth={612} displayedHeight={792} scale={1} rotation={0} />
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
  useToolStore.setState({ activeTool: "add-image" });
  useImageAddStore.setState({ path: "/tmp/logo.png" });
  useEditEpochStore.setState({ byDoc: {}, edited: {} });
});

describe("ImageAddLayer", () => {
  it("embeds the armed image into the dragged box and disarms", () => {
    const { container } = render(layer());
    const root = container.firstElementChild as Element;

    drag(root, [100, 100], [300, 250]);

    expect(mockAdd).toHaveBeenCalledTimes(1);
    const [doc, page, rect, path] = mockAdd.mock.calls[0];
    expect(doc).toBe(DOC);
    expect(page).toBe(0);
    expect(path).toBe("/tmp/logo.png");
    // Box → PDF rect: y flips (792 − screenY); ordered corners.
    expect(rect[0]).toBeCloseTo(100);
    expect(rect[2]).toBeCloseTo(300);
    // The tool disarmed after committing.
    expect(useImageAddStore.getState().path).toBeNull();
    expect(useToolStore.getState().activeTool).toBeNull();
  });

  it("renders nothing when no image is armed", () => {
    useImageAddStore.setState({ path: null });
    const { container } = render(layer());
    expect(container.firstChild).toBeNull();
  });
});
