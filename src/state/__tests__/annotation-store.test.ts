import { beforeEach, describe, expect, it } from "vitest";

import type { AnnotationDraft, NoteAnnotation } from "@/tools/_framework/types";

import { useAnnotationStore } from "../annotation-store";

const note = (id: string, content = ""): NoteAnnotation => ({
  id,
  type: "note",
  page: 0,
  point: { x: 100, y: 700 },
  content,
  author: "Ada",
  color: "#ffd200",
  opacity: 1,
  createdAt: 0,
});

const draft = (): AnnotationDraft => ({
  type: "rectangle",
  page: 0,
  rect: { x0: 0, y0: 0, x1: 10, y1: 10 },
  color: "#000000",
  opacity: 1,
  strokeWidth: 2,
});

beforeEach(() => {
  useAnnotationStore.setState({ byDoc: {}, draft: null, selectedId: null });
});

describe("annotation store", () => {
  it("add stamps id + createdAt, appends, and clears the draft", () => {
    const store = useAnnotationStore.getState();
    store.setDraft(draft());
    const a = store.add("doc-1", draft());

    expect(a.id).toBeTruthy();
    expect(a.createdAt).toBeGreaterThan(0);
    expect(useAnnotationStore.getState().byDoc["doc-1"]).toHaveLength(1);
    expect(useAnnotationStore.getState().draft).toBeNull();
  });

  it("update merges a patch into the matching annotation", () => {
    const a = useAnnotationStore.getState().add("doc-1", draft());
    useAnnotationStore.getState().update("doc-1", a.id, { color: "#ff0000" });
    expect(useAnnotationStore.getState().byDoc["doc-1"]?.[0]?.color).toBe("#ff0000");
  });

  it("remove deletes the annotation and clears the selection if it was selected", () => {
    const a = useAnnotationStore.getState().add("doc-1", draft());
    useAnnotationStore.getState().select(a.id);
    useAnnotationStore.getState().remove("doc-1", a.id);

    expect(useAnnotationStore.getState().byDoc["doc-1"]).toHaveLength(0);
    expect(useAnnotationStore.getState().selectedId).toBeNull();
  });

  it("isolates annotations per document and resets one without touching others", () => {
    useAnnotationStore.getState().add("doc-1", draft());
    useAnnotationStore.getState().add("doc-2", draft());

    useAnnotationStore.getState().resetDoc("doc-1");

    expect(useAnnotationStore.getState().byDoc["doc-1"]).toBeUndefined();
    expect(useAnnotationStore.getState().byDoc["doc-2"]).toHaveLength(1);
  });

  it("replaceNotes swaps the doc's notes but keeps other annotation types", () => {
    const store = useAnnotationStore.getState();
    const rect = store.add("doc-1", draft()); // a non-note annotation
    store.replaceNotes("doc-1", [note("n1", "hello"), note("n2")]);

    const list = useAnnotationStore.getState().byDoc["doc-1"] ?? [];
    expect(list.filter((a) => a.type === "note")).toHaveLength(2);
    // The rectangle survived the note replacement.
    expect(list.some((a) => a.id === rect.id)).toBe(true);

    // A second replace fully supersedes the previous notes.
    useAnnotationStore.getState().replaceNotes("doc-1", [note("n3")]);
    const after = useAnnotationStore.getState().byDoc["doc-1"] ?? [];
    expect(after.filter((a) => a.type === "note").map((a) => a.id)).toEqual(["n3"]);
    expect(after.some((a) => a.id === rect.id)).toBe(true);
  });

  it("tracks the live draft and selection", () => {
    useAnnotationStore.getState().setDraft(draft());
    expect(useAnnotationStore.getState().draft).not.toBeNull();
    useAnnotationStore.getState().select("some-id");
    expect(useAnnotationStore.getState().selectedId).toBe("some-id");
  });
});
