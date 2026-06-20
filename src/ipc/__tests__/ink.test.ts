// SPEC: P3-ANN-005 — the addInk IPC wrapper marshals (id, page, points,
// color/opacity/baseWidth) to the Rust command.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

import { addInk, type InkPoint } from "@/ipc/ink";
import { invoke } from "@/ipc/invoke";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
  mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });
});

describe("addInk", () => {
  it("marshals the points and style", async () => {
    const pts: InkPoint[] = [
      [100, 700, 0.5],
      [150, 690, 0.8],
      [200, 700, 0.3],
    ];
    await addInk("doc-1", 0, pts, "#1f6feb", 1, 2.5);
    expect(mockInvoke).toHaveBeenCalledWith("pdf_add_ink", {
      id: "doc-1",
      page: 0,
      points: pts,
      color: "#1f6feb",
      opacity: 1,
      baseWidth: 2.5,
    });
  });

  it("forwards opacity and base width", async () => {
    await addInk("doc-1", 2, [[0, 0, 0.5], [10, 10, 0.5]], "#000000", 0.6, 4);
    expect(mockInvoke).toHaveBeenCalledWith(
      "pdf_add_ink",
      expect.objectContaining({ page: 2, opacity: 0.6, baseWidth: 4 }),
    );
  });
});
