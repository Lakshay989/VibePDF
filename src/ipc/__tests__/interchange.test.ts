// SPEC: P3-ANN-010 — the XFDF interchange IPC wrappers marshal (id, path) to the
// Rust export/import commands.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

import { exportAnnotations, importAnnotations } from "@/ipc/interchange";
import { invoke } from "@/ipc/invoke";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
});

describe("exportAnnotations", () => {
  it("marshals the id + path and returns the count", async () => {
    mockInvoke.mockResolvedValue(3);
    const n = await exportAnnotations("doc-1", "/tmp/a.xfdf");
    expect(mockInvoke).toHaveBeenCalledWith("pdf_export_annotations", {
      id: "doc-1",
      path: "/tmp/a.xfdf",
    });
    expect(n).toBe(3);
  });
});

describe("importAnnotations", () => {
  it("marshals the id + path and returns the history state", async () => {
    mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });
    const h = await importAnnotations("doc-1", "/tmp/a.xfdf");
    expect(mockInvoke).toHaveBeenCalledWith("pdf_import_annotations", {
      id: "doc-1",
      path: "/tmp/a.xfdf",
    });
    expect(h).toEqual({ canUndo: true, canRedo: false });
  });
});
