// SPEC: P4-EDIT-006 (P4.C2) — the image-edit overlay: in edit-image mode it lays
// a hit-zone over each image; clicking selects it (showing the box + controls);
// Delete calls deleteImage. IPC mocked.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";

import type { ImageInfo } from "@/ipc/image-edit";

const { IMAGE } = vi.hoisted(() => ({
  IMAGE: {
    index: 0,
    bbox: [100, 400, 300, 600],
    matrix: [200, 0, 0, 200, 100, 400],
  } satisfies ImageInfo,
}));

vi.mock("@/ipc/image-edit", () => ({
  extractImages: vi.fn().mockResolvedValue([IMAGE]),
  transformImage: vi.fn().mockResolvedValue({ canUndo: true, canRedo: false }),
  deleteImage: vi.fn().mockResolvedValue({ canUndo: true, canRedo: false }),
}));

import { deleteImage, extractImages, transformImage } from "@/ipc/image-edit";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useToolStore } from "@/state/tool-store";
import { ImageEditLayer } from "@/view/image-edit-layer";

const DOC = "doc-1";
const mockDelete = vi.mocked(deleteImage);
const mockTransform = vi.mocked(transformImage);

const layer = () => (
  <ImageEditLayer documentId={DOC} page={0} displayedWidth={612} displayedHeight={792} scale={1} rotation={0} />
);

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});
beforeEach(() => {
  useToolStore.setState({ activeTool: "edit-image" });
  useEditEpochStore.setState({ byDoc: {}, edited: {} });
  vi.mocked(extractImages).mockResolvedValue([IMAGE]);
});

describe("ImageEditLayer", () => {
  it("selects an image on click and shows the controls", async () => {
    render(layer());
    fireEvent.click(await screen.findByTitle("Click to edit this image"));
    expect(screen.getByRole("button", { name: "Delete image" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Rotate image 90 degrees" })).toBeTruthy();
  });

  it("deletes the selected image via deleteImage", async () => {
    render(layer());
    fireEvent.click(await screen.findByTitle("Click to edit this image"));
    fireEvent.click(screen.getByRole("button", { name: "Delete image" }));
    expect(mockDelete).toHaveBeenCalledWith(DOC, 0, 0);
  });

  it("rotates the selected image via transformImage", async () => {
    render(layer());
    fireEvent.click(await screen.findByTitle("Click to edit this image"));
    fireEvent.click(screen.getByRole("button", { name: "Rotate image 90 degrees" }));
    expect(mockTransform).toHaveBeenCalledTimes(1);
    const [doc, page, index] = mockTransform.mock.calls[0];
    expect([doc, page, index]).toEqual([DOC, 0, 0]);
  });

  it("renders nothing when the edit-image tool is inactive", () => {
    useToolStore.setState({ activeTool: null });
    const { container } = render(layer());
    expect(container.firstChild).toBeNull();
  });
});
