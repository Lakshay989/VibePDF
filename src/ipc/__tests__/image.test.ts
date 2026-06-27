// SPEC: P4-EDIT-005 (P4.C1) — the addImage IPC wrapper marshals the box +
// file path to the Rust command and returns the history state.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

import { invoke } from "@/ipc/invoke";
import { addImage } from "@/ipc/image";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
});

describe("addImage", () => {
  it("marshals the page, rect, and image path", async () => {
    mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });

    const out = await addImage("doc-1", 1, [10, 20, 110, 120], "/tmp/logo.png");
    expect(mockInvoke).toHaveBeenCalledWith("pdf_add_image", {
      id: "doc-1",
      page: 1,
      rect: [10, 20, 110, 120],
      imagePath: "/tmp/logo.png",
    });
    expect(out).toEqual({ canUndo: true, canRedo: false });
  });
});
