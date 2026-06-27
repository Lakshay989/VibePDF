// SPEC: P4-EDIT-001 (P4.B1) — the replaceTextRun IPC wrapper marshals
// (id, page, runIndex, newText) to the Rust command and returns the history state.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

import { invoke } from "@/ipc/invoke";
import { deleteTextRun, replaceTextRun } from "@/ipc/text-edit";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
});

describe("replaceTextRun", () => {
  it("marshals the args and returns the history state", async () => {
    mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });

    const out = await replaceTextRun("doc-1", 2, 5, "new text");
    expect(mockInvoke).toHaveBeenCalledWith("pdf_replace_text_run", {
      id: "doc-1",
      page: 2,
      runIndex: 5,
      newText: "new text",
    });
    expect(out).toEqual({ canUndo: true, canRedo: false });
  });
});

describe("deleteTextRun", () => {
  it("marshals the document id, page, and run index", async () => {
    mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });

    const out = await deleteTextRun("doc-1", 3, 7);
    expect(mockInvoke).toHaveBeenCalledWith("pdf_delete_text_run", {
      id: "doc-1",
      page: 3,
      runIndex: 7,
    });
    expect(out).toEqual({ canUndo: true, canRedo: false });
  });
});
