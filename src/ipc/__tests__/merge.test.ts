// SPEC: P2-PAGE-008 — the mergeDocuments IPC wrapper marshals (paths, dest)
// and returns the backend's SaveOutcome.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

import { mergeDocuments } from "@/ipc/merge";
import { invoke } from "@/ipc/invoke";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
  mockInvoke.mockResolvedValue({ path: "/out.pdf", bytesWritten: 1, noOp: false });
});

describe("mergeDocuments", () => {
  it("marshals ordered paths and dest", async () => {
    await mergeDocuments(["/a.pdf", "/b.pdf"], "/out/merged.pdf");
    expect(mockInvoke).toHaveBeenCalledWith("pdf_merge_documents", {
      paths: ["/a.pdf", "/b.pdf"],
      dest: "/out/merged.pdf",
    });
  });

  it("returns the SaveOutcome from the backend", async () => {
    mockInvoke.mockResolvedValueOnce({
      path: "/out/merged.pdf",
      bytesWritten: 4242,
      noOp: false,
    });
    expect(await mergeDocuments(["/a.pdf", "/b.pdf"], "/out/merged.pdf")).toEqual({
      path: "/out/merged.pdf",
      bytesWritten: 4242,
      noOp: false,
    });
  });
});
