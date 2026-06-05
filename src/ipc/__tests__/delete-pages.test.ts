// SPEC: P2-PAGE-003 — the deletePages IPC wrapper marshals (id, pages)
// and returns the HistoryState the backend computed.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

import { deletePages } from "@/ipc/delete-pages";
import { invoke } from "@/ipc/invoke";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
  mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });
});

describe("deletePages", () => {
  it("marshals id and pages", async () => {
    await deletePages("doc-1", [1, 3]);
    expect(mockInvoke).toHaveBeenCalledWith("pdf_delete_pages", {
      id: "doc-1",
      pages: [1, 3],
    });
  });

  it("returns the HistoryState from the backend", async () => {
    mockInvoke.mockResolvedValueOnce({ canUndo: true, canRedo: true });
    const h = await deletePages("doc-1", [0]);
    expect(h).toEqual({ canUndo: true, canRedo: true });
  });
});
