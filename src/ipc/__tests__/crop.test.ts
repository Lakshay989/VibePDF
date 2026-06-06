// SPEC: P2-PAGE-009 — the cropPage IPC wrapper. The four edges go through
// as numbers when cropping, or all-null on reset (the Rust command reads
// Option<f32> for each and distinguishes crop from reset).

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

import { cropPage } from "@/ipc/crop";
import { invoke } from "@/ipc/invoke";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
  mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });
});

describe("cropPage", () => {
  it("marshals the four edges when cropping", async () => {
    await cropPage("doc-1", 0, { left: 50, bottom: 50, right: 562, top: 742 });
    expect(mockInvoke).toHaveBeenCalledWith("pdf_crop_page", {
      id: "doc-1",
      page: 0,
      left: 50,
      bottom: 50,
      right: 562,
      top: 742,
    });
  });

  it("sends all-null on reset (no rect)", async () => {
    await cropPage("doc-1", 2);
    expect(mockInvoke).toHaveBeenCalledWith("pdf_crop_page", {
      id: "doc-1",
      page: 2,
      left: null,
      bottom: null,
      right: null,
      top: null,
    });
  });

  it("returns the HistoryState from the backend", async () => {
    mockInvoke.mockResolvedValueOnce({ canUndo: true, canRedo: true });
    expect(await cropPage("doc-1", 0)).toEqual({ canUndo: true, canRedo: true });
  });
});
