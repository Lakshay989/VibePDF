// SPEC: P2-PAGE-007 — the splitDocument IPC wrapper marshals (id, mode,
// destDir, stem) and returns the backend's SplitOutcome.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

import { splitDocument } from "@/ipc/split";
import { invoke } from "@/ipc/invoke";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
  mockInvoke.mockResolvedValue({ files: [] });
});

describe("splitDocument", () => {
  it("marshals an everyN split", async () => {
    await splitDocument("doc-1", { kind: "everyN", n: 10 }, "/out", "book");
    expect(mockInvoke).toHaveBeenCalledWith("pdf_split_document", {
      id: "doc-1",
      mode: { kind: "everyN", n: 10 },
      destDir: "/out",
      stem: "book",
    });
  });

  it("marshals a bySize split with maxBytes", async () => {
    await splitDocument("doc-2", { kind: "bySize", maxBytes: 5242880 }, "/d", "s");
    expect(mockInvoke).toHaveBeenCalledWith("pdf_split_document", {
      id: "doc-2",
      mode: { kind: "bySize", maxBytes: 5242880 },
      destDir: "/d",
      stem: "s",
    });
  });

  it("returns the SplitOutcome from the backend", async () => {
    mockInvoke.mockResolvedValueOnce({
      files: [
        { path: "/out/book-001.pdf", bytesWritten: 10, noOp: false },
        { path: "/out/book-002.pdf", bytesWritten: 12, noOp: false },
      ],
    });
    const out = await splitDocument("doc-1", { kind: "bookmarks" }, "/out", "book");
    expect(out.files).toHaveLength(2);
    expect(out.files[0]?.path).toBe("/out/book-001.pdf");
  });
});
