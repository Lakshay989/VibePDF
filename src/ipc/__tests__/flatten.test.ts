// SPEC: P3-ANN-011 — the flattenAnnotations IPC wrapper marshals the document id
// to the Rust command and returns the new history state.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

import { flattenAnnotations } from "@/ipc/flatten";
import { invoke } from "@/ipc/invoke";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
  mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });
});

describe("flattenAnnotations", () => {
  it("marshals the document id", async () => {
    const h = await flattenAnnotations("doc-1");
    expect(mockInvoke).toHaveBeenCalledWith("pdf_flatten_annotations", { id: "doc-1" });
    expect(h).toEqual({ canUndo: true, canRedo: false });
  });
});
