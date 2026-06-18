// SPEC: P3-ANN-003 — the addFreeText IPC wrapper marshals (id, page, rect, text,
// style) to the Rust command and returns the HistoryState the actor computed.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

import { invoke } from "@/ipc/invoke";
import { addFreeText } from "@/ipc/freetext";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
  mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });
});

describe("addFreeText", () => {
  it("marshals id, page, rect, text, and style", async () => {
    await addFreeText("doc-1", 0, [100, 600, 320, 700], "Hi", "Times", 18, "#ff0000", true, false);
    expect(mockInvoke).toHaveBeenCalledWith("pdf_add_free_text", {
      id: "doc-1",
      page: 0,
      rect: [100, 600, 320, 700],
      text: "Hi",
      fontFamily: "Times",
      fontSize: 18,
      color: "#ff0000",
      bold: true,
      italic: false,
    });
  });

  it("returns the HistoryState from the backend", async () => {
    mockInvoke.mockResolvedValueOnce({ canUndo: true, canRedo: true });
    const h = await addFreeText("doc-1", 0, [0, 0, 1, 1], "x", "Helvetica", 12, "#000000", false, false);
    expect(h).toEqual({ canUndo: true, canRedo: true });
  });
});
