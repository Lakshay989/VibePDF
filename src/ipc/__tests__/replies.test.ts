// SPEC: P3-ANN-009 — the addReply IPC wrapper marshals (id, parentId, author,
// content) to the Rust command.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

import { invoke } from "@/ipc/invoke";
import { addReply } from "@/ipc/replies";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
  mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });
});

describe("addReply", () => {
  it("marshals the parent handle, author, and content", async () => {
    await addReply("doc-1", "parent-nm", "VibePDF User", "looks good");
    expect(mockInvoke).toHaveBeenCalledWith("pdf_add_reply", {
      id: "doc-1",
      parentId: "parent-nm",
      author: "VibePDF User",
      content: "looks good",
    });
  });
});
