// SPEC: P3-ANN-002 — the sticky-note IPC wrappers marshal their args to the
// Rust commands (camelCase → snake_case is Tauri's job) and return the
// HistoryState the actor computed (used to refresh Undo/Redo button state).

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

import { invoke } from "@/ipc/invoke";
import { addTextNote, deleteAnnotation, updateTextNote } from "@/ipc/notes";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
  mockInvoke.mockResolvedValue({ canUndo: true, canRedo: false });
});

describe("notes IPC", () => {
  it("addTextNote marshals id, noteId, page, point, content, author", async () => {
    await addTextNote("doc-1", "nm-1", 0, 120.5, 650, "hi", "Ada");
    expect(mockInvoke).toHaveBeenCalledWith("pdf_add_text_note", {
      id: "doc-1",
      noteId: "nm-1",
      page: 0,
      x: 120.5,
      y: 650,
      content: "hi",
      author: "Ada",
    });
  });

  it("updateTextNote marshals id, noteId, content", async () => {
    await updateTextNote("doc-1", "nm-1", "edited");
    expect(mockInvoke).toHaveBeenCalledWith("pdf_update_text_note", {
      id: "doc-1",
      noteId: "nm-1",
      content: "edited",
    });
  });

  it("deleteAnnotation marshals id, noteId", async () => {
    await deleteAnnotation("doc-1", "nm-1");
    expect(mockInvoke).toHaveBeenCalledWith("pdf_delete_annotation", {
      id: "doc-1",
      noteId: "nm-1",
    });
  });

  it("returns the HistoryState from the backend", async () => {
    mockInvoke.mockResolvedValueOnce({ canUndo: true, canRedo: true });
    const h = await addTextNote("doc-1", "nm-1", 0, 1, 2, "x", "A");
    expect(h).toEqual({ canUndo: true, canRedo: true });
  });
});
