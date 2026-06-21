// SPEC: P3-ANN-006 — the addStamp IPC wrapper marshals (id, page, rect, text,
// name, color, opacity) to the Rust command.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

import { invoke } from "@/ipc/invoke";
import { addStamp } from "@/ipc/stamps";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
  mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });
});

describe("addStamp", () => {
  it("marshals the rect and stamp fields", async () => {
    await addStamp("doc-1", 0, [100, 600, 250, 646], "APPROVED", "Approved", "#1e8449", 1);
    expect(mockInvoke).toHaveBeenCalledWith("pdf_add_stamp", {
      id: "doc-1",
      page: 0,
      rect: [100, 600, 250, 646],
      text: "APPROVED",
      name: "Approved",
      color: "#1e8449",
      opacity: 1,
    });
  });
});
