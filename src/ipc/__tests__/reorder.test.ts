// SPEC: P2-PAGE-002 — the reorderPages IPC wrapper marshals (id, newOrder)
// and returns the backend's HistoryState.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

import { reorderPages } from "@/ipc/reorder";
import { invoke } from "@/ipc/invoke";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
  mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });
});

describe("reorderPages", () => {
  it("marshals id and newOrder", async () => {
    await reorderPages("doc-1", [1, 2, 0]);
    expect(mockInvoke).toHaveBeenCalledWith("pdf_reorder_pages", {
      id: "doc-1",
      newOrder: [1, 2, 0],
    });
  });

  it("returns the HistoryState from the backend", async () => {
    mockInvoke.mockResolvedValueOnce({ canUndo: true, canRedo: false });
    expect(await reorderPages("doc-1", [0, 1])).toEqual({ canUndo: true, canRedo: false });
  });
});
