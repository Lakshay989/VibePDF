// SPEC: P3-ANN-007 — the addMeasure IPC wrapper marshals (id, page, kind, points,
// color, label, opacity, strokeWidth, unitsPerPoint, unit) to the Rust command;
// readMeasureCalibration marshals the id (P3.C4b).

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

import { invoke } from "@/ipc/invoke";
import { addMeasure, readMeasureCalibration } from "@/ipc/measure";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
  mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });
});

describe("addMeasure", () => {
  it("marshals the kind, points, label, and calibration", async () => {
    await addMeasure("doc-1", 0, "distance", [[100, 700], [300, 700]], "#1f6feb", "4 m", 1, 1.5, 0.02, "m");
    expect(mockInvoke).toHaveBeenCalledWith("pdf_add_measure", {
      id: "doc-1",
      page: 0,
      kind: "distance",
      points: [[100, 700], [300, 700]],
      color: "#1f6feb",
      label: "4 m",
      opacity: 1,
      strokeWidth: 1.5,
      unitsPerPoint: 0.02,
      unit: "m",
    });
  });
});

describe("readMeasureCalibration", () => {
  it("marshals the document id and returns the calibration", async () => {
    mockInvoke.mockResolvedValue({ unitsPerPoint: 0.5, unit: "ft" });
    const cal = await readMeasureCalibration("doc-1");
    expect(mockInvoke).toHaveBeenCalledWith("pdf_read_measure_calibration", { id: "doc-1" });
    expect(cal).toEqual({ unitsPerPoint: 0.5, unit: "ft" });
  });
});
