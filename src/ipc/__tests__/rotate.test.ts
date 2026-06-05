// SPEC: P2-PAGE-001 — the rotatePages IPC wrapper marshals (id, pages,
// degrees) and returns the HistoryState the backend computed (used to
// refresh the Undo/Redo button state).

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

import { invoke } from "@/ipc/invoke";
import { rotatePages } from "@/ipc/rotate";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
  mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });
});

describe("rotatePages", () => {
  it("marshals id, pages, and degrees", async () => {
    await rotatePages("doc-1", [0, 2], 90);
    expect(mockInvoke).toHaveBeenCalledWith("pdf_rotate_pages", {
      id: "doc-1",
      pages: [0, 2],
      degrees: 90,
    });
  });

  it("returns the HistoryState from the backend", async () => {
    mockInvoke.mockResolvedValueOnce({ canUndo: true, canRedo: true });
    const h = await rotatePages("doc-1", [0], -90);
    expect(h).toEqual({ canUndo: true, canRedo: true });
  });
});
