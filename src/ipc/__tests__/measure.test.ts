// SPEC: P3-ANN-007 — the addMeasure IPC wrapper marshals (id, page, kind, points,
// color, label, opacity, strokeWidth) to the Rust command.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

import { invoke } from "@/ipc/invoke";
import { addMeasure } from "@/ipc/measure";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
  mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });
});

describe("addMeasure", () => {
  it("marshals the kind, points, and value label", async () => {
    await addMeasure("doc-1", 0, "distance", [[100, 700], [300, 700]], "#1f6feb", "4 m", 1, 1.5);
    expect(mockInvoke).toHaveBeenCalledWith("pdf_add_measure", {
      id: "doc-1",
      page: 0,
      kind: "distance",
      points: [[100, 700], [300, 700]],
      color: "#1f6feb",
      label: "4 m",
      opacity: 1,
      strokeWidth: 1.5,
    });
  });
});
