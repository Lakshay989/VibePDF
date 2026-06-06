// SPEC: P2-PAGE-004 — the insertBlankPage IPC wrapper marshals
// (id, index, width, height) and returns the backend's HistoryState. The
// width/height ?? null matters: the Rust command reads Option<f32>, and
// undefined would not deserialize.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

import { insertBlankPage } from "@/ipc/insert-blank";
import { invoke } from "@/ipc/invoke";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
  mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });
});

describe("insertBlankPage", () => {
  it("marshals index with null size when omitted (inherit)", async () => {
    await insertBlankPage("doc-1", 2);
    expect(mockInvoke).toHaveBeenCalledWith("pdf_insert_blank_page", {
      id: "doc-1",
      index: 2,
      width: null,
      height: null,
    });
  });

  it("passes an explicit size through", async () => {
    await insertBlankPage("doc-1", 0, { width: 612, height: 792 });
    expect(mockInvoke).toHaveBeenCalledWith("pdf_insert_blank_page", {
      id: "doc-1",
      index: 0,
      width: 612,
      height: 792,
    });
  });

  it("returns the HistoryState from the backend", async () => {
    mockInvoke.mockResolvedValueOnce({ canUndo: true, canRedo: true });
    const h = await insertBlankPage("doc-1", 1);
    expect(h).toEqual({ canUndo: true, canRedo: true });
  });
});
