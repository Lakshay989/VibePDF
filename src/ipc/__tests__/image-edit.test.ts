// SPEC: P4-EDIT-006 (P4.C2) — the image-edit IPC wrappers marshal to the Rust
// commands and return their typed results.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

import { invoke } from "@/ipc/invoke";
import { deleteImage, extractImages, transformImage } from "@/ipc/image-edit";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
});

describe("image-edit IPC", () => {
  it("extractImages marshals (id, page)", async () => {
    mockInvoke.mockResolvedValue([{ index: 0, bbox: [1, 2, 3, 4], matrix: [1, 0, 0, 1, 0, 0] }]);
    const out = await extractImages("doc-1", 2);
    expect(mockInvoke).toHaveBeenCalledWith("pdf_extract_images", { id: "doc-1", page: 2 });
    expect(out[0].index).toBe(0);
  });

  it("transformImage marshals (id, page, index, matrix)", async () => {
    mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });
    await transformImage("doc-1", 0, 1, [2, 0, 0, 3, 10, 20]);
    expect(mockInvoke).toHaveBeenCalledWith("pdf_transform_image", {
      id: "doc-1",
      page: 0,
      index: 1,
      matrix: [2, 0, 0, 3, 10, 20],
    });
  });

  it("deleteImage marshals (id, page, index)", async () => {
    mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });
    await deleteImage("doc-1", 0, 1);
    expect(mockInvoke).toHaveBeenCalledWith("pdf_delete_image", { id: "doc-1", page: 0, index: 1 });
  });
});
