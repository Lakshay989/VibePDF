// SPEC: P3-ANN-001 / P3-ANN-008 — the annotation IPC wrappers marshal their
// args to the Rust commands and return what the actor computed.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

import { invoke } from "@/ipc/invoke";
import { addTextMarkup, clearTextMarkup, readAnnotations } from "@/ipc/annotations";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
  mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });
});

describe("annotation IPC", () => {
  it("addTextMarkup marshals id, page, subtype, quads, colour, opacity", async () => {
    const quads: [number, number, number, number, number, number, number, number][] = [
      [10, 20, 110, 20, 10, 8, 110, 8],
    ];
    await addTextMarkup("doc-1", 0, "highlight", quads, "#ffd400", 1);
    expect(mockInvoke).toHaveBeenCalledWith("pdf_add_text_markup", {
      id: "doc-1",
      page: 0,
      subtype: "highlight",
      quads,
      color: "#ffd400",
      opacity: 1,
    });
  });

  it("clearTextMarkup marshals id", async () => {
    await clearTextMarkup("doc-1");
    expect(mockInvoke).toHaveBeenCalledWith("pdf_clear_text_markup", { id: "doc-1" });
  });

  it("readAnnotations marshals id and returns the rows", async () => {
    const rows = [
      { id: "5 0", page: 0, kind: "note", rect: [1, 2, 3, 4], contents: "hi", author: "Ada", modified: 1 },
    ];
    mockInvoke.mockResolvedValueOnce(rows);
    const out = await readAnnotations("doc-1");
    expect(mockInvoke).toHaveBeenCalledWith("pdf_read_annotations", { id: "doc-1" });
    expect(out).toEqual(rows);
  });
});
