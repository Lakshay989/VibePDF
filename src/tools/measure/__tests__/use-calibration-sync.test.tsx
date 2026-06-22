// SPEC: P3-ANN-007 (P3.C4b) — useCalibrationSync seeds the measure store from
// the document's persisted /Measure on open, but never clobbers a calibration
// the user set this session.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, waitFor } from "@testing-library/react";

vi.mock("@/ipc/measure", () => ({ readMeasureCalibration: vi.fn() }));

import { readMeasureCalibration } from "@/ipc/measure";
import { useMeasureStore } from "@/state/measure-store";
import { useCalibrationSync } from "@/tools/measure/use-calibration-sync";

const mockRead = vi.mocked(readMeasureCalibration);

function Probe({ id }: { id: string }) {
  useCalibrationSync(id);
  return null;
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});
beforeEach(() => {
  useMeasureStore.setState({ calibration: {} });
});

describe("useCalibrationSync", () => {
  it("seeds the store from the document's saved calibration", async () => {
    mockRead.mockResolvedValue({ unitsPerPoint: 0.5, unit: "ft" });
    render(<Probe id="doc-1" />);
    await waitFor(() =>
      expect(useMeasureStore.getState().calibration["doc-1"]).toEqual({ unitsPerPoint: 0.5, unit: "ft" }),
    );
  });

  it("does not clobber a calibration the user set this session", async () => {
    useMeasureStore.setState({ calibration: { "doc-1": { unitsPerPoint: 0.25, unit: "in" } } });
    mockRead.mockResolvedValue({ unitsPerPoint: 0.5, unit: "ft" });
    render(<Probe id="doc-1" />);
    // Give the async read a tick to resolve, then confirm the session value stuck.
    await waitFor(() => expect(mockRead).toHaveBeenCalledWith("doc-1"));
    expect(useMeasureStore.getState().calibration["doc-1"]).toEqual({ unitsPerPoint: 0.25, unit: "in" });
  });

  it("does nothing when the document has no calibration", async () => {
    mockRead.mockResolvedValue(null);
    render(<Probe id="doc-1" />);
    await waitFor(() => expect(mockRead).toHaveBeenCalledWith("doc-1"));
    expect(useMeasureStore.getState().calibration["doc-1"]).toBeUndefined();
  });
});
