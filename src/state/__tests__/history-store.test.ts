// SPEC: P2-PAGE-003 / session history — the frontend mirror of per-
// document undo/redo availability. The actor is the source of truth;
// this store only drives button state. Tested at the store-action level
// (no render needed).

import { beforeEach, describe, expect, it } from "vitest";

import { useHistoryStore } from "@/state/history-store";

beforeEach(() => {
  useHistoryStore.setState({ byDoc: {} });
});

describe("history-store", () => {
  it("records availability per document", () => {
    useHistoryStore.getState().setHistory("d1", { canUndo: true, canRedo: false });
    expect(useHistoryStore.getState().byDoc["d1"]).toEqual({
      canUndo: true,
      canRedo: false,
    });
  });

  it("overwrites the previous state for a document", () => {
    const { setHistory } = useHistoryStore.getState();
    setHistory("d1", { canUndo: true, canRedo: false });
    setHistory("d1", { canUndo: false, canRedo: true });
    expect(useHistoryStore.getState().byDoc["d1"]).toEqual({
      canUndo: false,
      canRedo: true,
    });
  });

  it("keeps documents independent", () => {
    const { setHistory } = useHistoryStore.getState();
    setHistory("d1", { canUndo: true, canRedo: false });
    setHistory("d2", { canUndo: false, canRedo: true });
    expect(useHistoryStore.getState().byDoc["d1"]).toEqual({
      canUndo: true,
      canRedo: false,
    });
    expect(useHistoryStore.getState().byDoc["d2"]).toEqual({
      canUndo: false,
      canRedo: true,
    });
  });

  it("clearHistory removes a document's entry", () => {
    const { setHistory, clearHistory } = useHistoryStore.getState();
    setHistory("d1", { canUndo: true, canRedo: true });
    clearHistory("d1");
    expect("d1" in useHistoryStore.getState().byDoc).toBe(false);
  });

  it("clearHistory on an unknown id is a no-op (byDoc reference preserved)", () => {
    const before = useHistoryStore.getState().byDoc;
    useHistoryStore.getState().clearHistory("nope");
    expect(useHistoryStore.getState().byDoc).toBe(before);
  });
});
