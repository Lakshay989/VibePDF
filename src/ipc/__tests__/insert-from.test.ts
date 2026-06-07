// SPEC: P2-PAGE-005 — the insertPagesFromPdf IPC wrapper marshals
// (id, sourcePath, pages, index) and returns the backend's HistoryState.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

import { insertPagesFromPdf } from "@/ipc/insert-from";
import { invoke } from "@/ipc/invoke";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
  mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });
});

describe("insertPagesFromPdf", () => {
  it("marshals id, sourcePath, pages, and index", async () => {
    await insertPagesFromPdf("doc-1", "/b.pdf", [0, 2], 3);
    expect(mockInvoke).toHaveBeenCalledWith("pdf_insert_from_pdf", {
      id: "doc-1",
      sourcePath: "/b.pdf",
      pages: [0, 2],
      index: 3,
    });
  });

  it("returns the HistoryState from the backend", async () => {
    mockInvoke.mockResolvedValueOnce({ canUndo: true, canRedo: false });
    expect(await insertPagesFromPdf("doc-1", "/b.pdf", [0], 0)).toEqual({
      canUndo: true,
      canRedo: false,
    });
  });
});
