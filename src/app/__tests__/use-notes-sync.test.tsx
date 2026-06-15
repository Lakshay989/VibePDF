// SPEC: P3-ANN-002 (re-openable, P3.B2b) — useNotesSync projects the PDF's
// sticky notes into the overlay store on open and re-syncs on every edit-epoch
// bump (undo/redo). IPC is mocked — this asserts the hook's wiring (read → map →
// replaceNotes), not the Rust read path (covered by cos / text_note tests).

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";

vi.mock("@/ipc/notes", () => ({ readTextNotes: vi.fn() }));

import { readTextNotes, type NoteData } from "@/ipc/notes";
import { useNotesSync } from "@/app/use-notes-sync";
import { useAnnotationStore } from "@/state/annotation-store";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { NOTE_COLOR } from "@/tools/sticky-note/note-tool";

const DOC = "doc-1";
const mockRead = vi.mocked(readTextNotes);

const row = (nm: string, content = "hi"): NoteData => ({
  nm,
  page: 0,
  x: 100,
  y: 700,
  content,
  author: "Ada",
});

const notesInStore = () =>
  (useAnnotationStore.getState().byDoc[DOC] ?? []).filter((a) => a.type === "note");

afterEach(() => vi.clearAllMocks());
beforeEach(() => {
  useAnnotationStore.setState({ byDoc: {}, draft: null, selectedId: null });
  useEditEpochStore.setState({ byDoc: {}, edited: {} });
  mockRead.mockResolvedValue([]);
});

describe("useNotesSync", () => {
  it("seeds the store from the PDF when a document opens", async () => {
    mockRead.mockResolvedValueOnce([row("n1", "hello")]);
    renderHook(() => useNotesSync(DOC));

    await waitFor(() => expect(notesInStore()).toHaveLength(1));
    const n = notesInStore()[0];
    expect(n).toMatchObject({
      id: "n1",
      type: "note",
      page: 0,
      point: { x: 100, y: 700 },
      content: "hello",
      author: "Ada",
      color: NOTE_COLOR,
      opacity: 1,
    });
    expect(mockRead).toHaveBeenCalledWith(DOC);
  });

  it("does nothing without a document id", () => {
    renderHook(() => useNotesSync(undefined));
    expect(mockRead).not.toHaveBeenCalled();
  });

  it("re-syncs when the edit epoch bumps (undo/redo)", async () => {
    mockRead.mockResolvedValueOnce([row("n1")]); // initial open: one note
    renderHook(() => useNotesSync(DOC));
    await waitFor(() => expect(notesInStore()).toHaveLength(1));

    // An undo removed the note in the actor; the epoch bump drives a re-read.
    mockRead.mockResolvedValueOnce([]);
    act(() => useEditEpochStore.getState().bumpEpoch(DOC));

    await waitFor(() => expect(notesInStore()).toHaveLength(0));
    expect(mockRead).toHaveBeenCalledTimes(2);
  });

  it("replaces notes rather than appending on re-sync", async () => {
    mockRead.mockResolvedValueOnce([row("n1", "first")]);
    renderHook(() => useNotesSync(DOC));
    await waitFor(() => expect(notesInStore()).toHaveLength(1));

    mockRead.mockResolvedValueOnce([row("n2", "second")]);
    act(() => useEditEpochStore.getState().bumpEpoch(DOC));

    await waitFor(() => expect(notesInStore().map((a) => a.id)).toEqual(["n2"]));
  });
});
