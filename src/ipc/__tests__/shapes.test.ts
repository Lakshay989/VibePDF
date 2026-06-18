// SPEC: P3-ANN-004 — the addShape IPC wrapper marshals (id, page, kind, rect,
// stroke/fill/opacity/width) to the Rust command and returns the HistoryState.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

import { invoke } from "@/ipc/invoke";
import { addShape } from "@/ipc/shapes";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
  mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });
});

describe("addShape", () => {
  it("marshals a filled rectangle", async () => {
    await addShape("doc-1", 0, "rectangle", [100, 600, 300, 700], "#ff0000", "#ffeeee", 1, 2);
    expect(mockInvoke).toHaveBeenCalledWith("pdf_add_shape", {
      id: "doc-1",
      page: 0,
      kind: "rectangle",
      rect: [100, 600, 300, 700],
      strokeColor: "#ff0000",
      fillColor: "#ffeeee",
      opacity: 1,
      strokeWidth: 2,
    });
  });

  it("passes a null fill for an unfilled shape", async () => {
    await addShape("doc-1", 1, "ellipse", [0, 0, 10, 10], "#000000", null, 0.5, 3);
    expect(mockInvoke).toHaveBeenCalledWith("pdf_add_shape", {
      id: "doc-1",
      page: 1,
      kind: "ellipse",
      rect: [0, 0, 10, 10],
      strokeColor: "#000000",
      fillColor: null,
      opacity: 0.5,
      strokeWidth: 3,
    });
  });

  it("returns the HistoryState from the backend", async () => {
    mockInvoke.mockResolvedValueOnce({ canUndo: true, canRedo: true });
    const h = await addShape("doc-1", 0, "rectangle", [0, 0, 1, 1], "#000000", null, 1, 1);
    expect(h).toEqual({ canUndo: true, canRedo: true });
  });
});
