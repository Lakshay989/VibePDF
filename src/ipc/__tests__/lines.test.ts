// SPEC: P3-ANN-004 — the addLine IPC wrapper marshals (id, page, endpoints,
// arrow, stroke/opacity/width) to the Rust command and returns the HistoryState.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

import { invoke } from "@/ipc/invoke";
import { addLine } from "@/ipc/lines";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
  mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });
});

describe("addLine", () => {
  it("marshals endpoints + the arrow flag + style", async () => {
    await addLine("doc-1", 0, 100, 700, 300, 650, true, "#0000ff", 0.9, 3);
    expect(mockInvoke).toHaveBeenCalledWith("pdf_add_line", {
      id: "doc-1",
      page: 0,
      x1: 100,
      y1: 700,
      x2: 300,
      y2: 650,
      arrow: true,
      strokeColor: "#0000ff",
      opacity: 0.9,
      strokeWidth: 3,
    });
  });

  it("returns the HistoryState from the backend", async () => {
    mockInvoke.mockResolvedValueOnce({ canUndo: true, canRedo: true });
    const h = await addLine("doc-1", 0, 0, 0, 10, 10, false, "#000000", 1, 1);
    expect(h).toEqual({ canUndo: true, canRedo: true });
  });
});
