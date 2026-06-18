// SPEC: P3-ANN-002 (P3.B2a) — the sticky-note overlay: it draws note icons from
// the store, places a note (persisting through the actor) when the note tool is
// active, and edits/deletes via its popup. IPC is mocked — these assert the
// overlay's wiring (store + IPC calls), not the Rust write path (covered by the
// Rust `text_note` / `cos` tests).

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render } from "@testing-library/react";

vi.mock("@/ipc/notes", () => ({
  addTextNote: vi.fn().mockResolvedValue({ canUndo: true, canRedo: false }),
  updateTextNote: vi.fn().mockResolvedValue({ canUndo: true, canRedo: false }),
  deleteAnnotation: vi.fn().mockResolvedValue({ canUndo: true, canRedo: false }),
}));

import { addTextNote, deleteAnnotation, updateTextNote } from "@/ipc/notes";
import { useAnnotationStore } from "@/state/annotation-store";
import { useHistoryStore } from "@/state/history-store";
import { useToolStore } from "@/state/tool-store";
import type { Annotation } from "@/tools/_framework";
import { NoteLayer } from "@/view/note-layer";

const DOC = "doc-1";

// Letter (612×792), 1× scale, no rotation → PDF (x,y) maps to screen (x, 792−y).
const layer = () => (
  <NoteLayer
    documentId={DOC}
    page={0}
    displayedWidth={612}
    displayedHeight={792}
    scale={1}
    rotation={0}
  />
);

const noteAnn = (content = "hello"): Annotation => ({
  id: "n1",
  type: "note",
  page: 0,
  point: { x: 100, y: 700 },
  content,
  author: "Ada",
  color: "#ffd200",
  opacity: 1,
  createdAt: 1,
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});
beforeEach(() => {
  useAnnotationStore.setState({ byDoc: {}, draft: null, selectedId: null });
  useToolStore.setState({ activeTool: null });
  useHistoryStore.setState({ byDoc: {} });
});

describe("NoteLayer", () => {
  it("draws a note icon for each stored note on the page", () => {
    useAnnotationStore.setState({ byDoc: { [DOC]: [noteAnn()] }, draft: null, selectedId: null });
    const { container } = render(layer());
    const icon = container.querySelector('button[aria-label="Note: hello"]');
    expect(icon).not.toBeNull();
    // PDF (100,700) → screen x=100, y=792−700=92; icon bottom-left anchored,
    // so top = 92 − 18.
    expect((icon as HTMLElement).style.left).toBe("100px");
    expect((icon as HTMLElement).style.top).toBe("74px");
  });

  it("opens the popup when an icon is clicked", () => {
    useAnnotationStore.setState({ byDoc: { [DOC]: [noteAnn()] }, draft: null, selectedId: null });
    const { container } = render(layer());
    expect(container.querySelector('[role="dialog"]')).toBeNull();

    fireEvent.click(container.querySelector('button[aria-label="Note: hello"]') as Element);
    const popup = container.querySelector('[role="dialog"]');
    expect(popup).not.toBeNull();
    expect((popup?.querySelector("textarea") as HTMLTextAreaElement).value).toBe("hello");
    // Regression: the popup MUST opt back into pointer events — the NoteLayer
    // container is `pointer-events: none` when idle, so without this the textarea
    // and Save would be click-through (you couldn't type or save).
    expect((popup as HTMLElement).style.pointerEvents).toBe("auto");
  });

  it("places + persists a note when the note tool is active", () => {
    useToolStore.setState({ activeTool: "sticky-note" });
    const { container } = render(layer());
    const root = container.firstElementChild as Element;

    fireEvent.pointerDown(root, { clientX: 100, clientY: 200, pointerId: 1 });

    const list = useAnnotationStore.getState().byDoc[DOC] ?? [];
    expect(list).toHaveLength(1);
    const placed = list[0];
    if (!placed || placed.type !== "note") throw new Error("expected a note");
    // screen (100,200) → PDF (100, 592).
    expect(placed.point).toEqual({ x: 100, y: 592 });
    // Persisted via the actor with the store id as the /NM.
    expect(addTextNote).toHaveBeenCalledWith(DOC, placed.id, 0, 100, 592, "", placed.author);
    // Tool drops back to idle and the popup opens for editing.
    expect(useToolStore.getState().activeTool).toBeNull();
    expect(container.querySelector('[role="dialog"]')).not.toBeNull();
  });

  it("saves an edit: updates the store and calls updateTextNote", () => {
    useAnnotationStore.setState({ byDoc: { [DOC]: [noteAnn("")] }, draft: null, selectedId: null });
    const { container } = render(layer());
    fireEvent.click(container.querySelector('button[aria-label="Sticky note"]') as Element);

    const textarea = container.querySelector("textarea") as HTMLTextAreaElement;
    fireEvent.change(textarea, { target: { value: "typed body" } });
    fireEvent.click(container.querySelector('button[aria-label="Save note"]') as Element);

    expect(updateTextNote).toHaveBeenCalledWith(DOC, "n1", "typed body");
    const stored = useAnnotationStore.getState().byDoc[DOC]?.[0];
    if (!stored || stored.type !== "note") throw new Error("expected a note");
    expect(stored.content).toBe("typed body");
    // Popup closes after save.
    expect(container.querySelector('[role="dialog"]')).toBeNull();
  });

  it("deletes a note: removes from the store and calls deleteAnnotation", () => {
    useAnnotationStore.setState({ byDoc: { [DOC]: [noteAnn()] }, draft: null, selectedId: null });
    const { container } = render(layer());
    fireEvent.click(container.querySelector('button[aria-label="Note: hello"]') as Element);
    fireEvent.click(container.querySelector('button[aria-label="Delete note"]') as Element);

    expect(deleteAnnotation).toHaveBeenCalledWith(DOC, "n1");
    expect(useAnnotationStore.getState().byDoc[DOC]).toHaveLength(0);
    expect(container.querySelector('[role="dialog"]')).toBeNull();
  });
});
