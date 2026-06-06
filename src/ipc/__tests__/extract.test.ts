// SPEC: P2-PAGE-006 — the extractPages IPC wrapper marshals (id, pages,
// dest) and returns the backend's SaveOutcome.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

import { extractPages } from "@/ipc/extract";
import { invoke } from "@/ipc/invoke";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
  mockInvoke.mockResolvedValue({ path: "/x.pdf", bytesWritten: 1, noOp: false });
});

describe("extractPages", () => {
  it("marshals id, pages, and dest", async () => {
    await extractPages("doc-1", [0, 2], "/out/extracted.pdf");
    expect(mockInvoke).toHaveBeenCalledWith("pdf_extract_pages", {
      id: "doc-1",
      pages: [0, 2],
      dest: "/out/extracted.pdf",
    });
  });

  it("returns the SaveOutcome from the backend", async () => {
    mockInvoke.mockResolvedValueOnce({
      path: "/out.pdf",
      bytesWritten: 999,
      noOp: false,
    });
    expect(await extractPages("doc-1", [0], "/out.pdf")).toEqual({
      path: "/out.pdf",
      bytesWritten: 999,
      noOp: false,
    });
  });
});
