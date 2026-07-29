// Edit-preview pipeline — the per-document "something changed" signal.
// The main view and thumbnails subscribe and reload when a doc's epoch
// changes. Tested at the store-action level.

import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  isDocEdited,
  useDebouncedDocEpoch,
  useEditEpochStore,
} from "@/state/edit-epoch-store";

beforeEach(() => {
  useEditEpochStore.setState({ byDoc: {}, edited: {} });
});

describe("edit-epoch-store", () => {
  it("is absent (treated as 0) until the first bump, then increments", () => {
    expect(useEditEpochStore.getState().byDoc["d1"]).toBeUndefined();
    useEditEpochStore.getState().bumpEpoch("d1");
    expect(useEditEpochStore.getState().byDoc["d1"]).toBe(1);
    useEditEpochStore.getState().bumpEpoch("d1");
    expect(useEditEpochStore.getState().byDoc["d1"]).toBe(2);
  });

  it("keeps documents independent", () => {
    const { bumpEpoch } = useEditEpochStore.getState();
    bumpEpoch("d1");
    bumpEpoch("d1");
    bumpEpoch("d2");
    expect(useEditEpochStore.getState().byDoc["d1"]).toBe(2);
    expect(useEditEpochStore.getState().byDoc["d2"]).toBe(1);
  });

  it("tracks the edited flag: false until marked or bumped", () => {
    expect(isDocEdited("d1")).toBe(false);
    // A rotate marks edited *without* bumping the epoch.
    useEditEpochStore.getState().markEdited("d1");
    expect(isDocEdited("d1")).toBe(true);
    expect(useEditEpochStore.getState().byDoc["d1"]).toBeUndefined();
  });

  it("bumpEpoch also marks the doc edited (delete/undo/redo)", () => {
    expect(isDocEdited("d2")).toBe(false);
    useEditEpochStore.getState().bumpEpoch("d2");
    expect(isDocEdited("d2")).toBe(true);
    expect(isDocEdited("d1")).toBe(false); // independent
  });
});

// SPEC: NFR-PERF-005 — the debounced epoch holds steady during rapid edits so
// the main view reloads (and blanks) at most once per pause.
describe("useDebouncedDocEpoch", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("holds steady while edits keep coming, then settles after the delay", () => {
    vi.useFakeTimers();
    const { result } = renderHook(() => useDebouncedDocEpoch("d1", 900));
    expect(result.current).toBe(0);

    // Three quick edits within the window: the debounced value stays at 0.
    act(() => {
      useEditEpochStore.getState().bumpEpoch("d1");
      useEditEpochStore.getState().bumpEpoch("d1");
    });
    act(() => vi.advanceTimersByTime(500));
    act(() => {
      useEditEpochStore.getState().bumpEpoch("d1"); // resets the timer
    });
    act(() => vi.advanceTimersByTime(500));
    expect(result.current).toBe(0);

    // Editing pauses past the delay → the debounced value catches up to 3.
    act(() => vi.advanceTimersByTime(900));
    expect(result.current).toBe(3);
  });
});
