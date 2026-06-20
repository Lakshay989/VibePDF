// SPEC: P3-ANN-004 — the addPolygon IPC wrapper marshals (id, page, closed,
// points, stroke/fill/opacity/width) to the Rust command.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

import { invoke } from "@/ipc/invoke";
import { addPolygon } from "@/ipc/polygons";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
  mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });
});

describe("addPolygon", () => {
  it("marshals closed, the vertices, and style", async () => {
    const pts: [number, number][] = [
      [100, 700],
      [200, 700],
      [150, 620],
    ];
    await addPolygon("doc-1", 0, true, pts, "#ff0000", "#ffeeee", 1, 2);
    expect(mockInvoke).toHaveBeenCalledWith("pdf_add_polygon", {
      id: "doc-1",
      page: 0,
      closed: true,
      points: pts,
      strokeColor: "#ff0000",
      fillColor: "#ffeeee",
      opacity: 1,
      strokeWidth: 2,
    });
  });

  it("passes a null fill", async () => {
    await addPolygon("doc-1", 1, true, [[0, 0], [10, 0], [5, 10]], "#000000", null, 0.5, 3);
    expect(mockInvoke).toHaveBeenCalledWith(
      "pdf_add_polygon",
      expect.objectContaining({ fillColor: null, opacity: 0.5, strokeWidth: 3 }),
    );
  });
});
