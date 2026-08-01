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
  useEditEpochStore.setState({ byDoc: {}, bakeByDoc: {}, pendingBake: {}, edited: {} });
});

describe("soft vs hard bumps", () => {
  it("a hard bump advances both raw and bake, and returns the new bake", () => {
    const bake = useEditEpochStore.getState().bumpEpoch("d1");
    expect(useEditEpochStore.getState().byDoc["d1"]).toBe(1);
    expect(useEditEpochStore.getState().bakeByDoc["d1"]).toBe(1);
    expect(bake).toBe(1);
    expect(isDocEdited("d1")).toBe(true);
  });

  it("a soft bump advances raw only, and returns the *next* bake (unchanged)", () => {
    const tie = useEditEpochStore.getState().bumpEpochSoft("d1");
    expect(useEditEpochStore.getState().byDoc["d1"]).toBe(1);
    expect(useEditEpochStore.getState().bakeByDoc["d1"]).toBeUndefined(); // no bake yet
    expect(tie).toBe(1); // the bake that will eventually include this soft edit
    expect(isDocEdited("d1")).toBe(true);
  });

  it("soft edits pile onto the same pending bake until one lands", () => {
    const s = useEditEpochStore.getState();
    expect(s.bumpEpochSoft("d1")).toBe(1);
    expect(s.bumpEpochSoft("d1")).toBe(1); // still the same next-bake
    expect(useEditEpochStore.getState().byDoc["d1"]).toBe(2);
    // The idle backstop / a hard edit advances the bake to that value.
    useEditEpochStore.getState().bumpBake("d1");
    expect(useEditEpochStore.getState().bakeByDoc["d1"]).toBe(1);
  });

  it("a hard edit after soft edits bakes them: bake catches up by one", () => {
    const s = useEditEpochStore.getState();
    s.bumpEpochSoft("d1"); // raw 1, bake 0, tie→1
    s.bumpEpochSoft("d1"); // raw 2, bake 0, tie→1
    const bake = useEditEpochStore.getState().bumpEpoch("d1"); // raw 3, bake 1
    expect(bake).toBe(1);
    expect(useEditEpochStore.getState().byDoc["d1"]).toBe(3);
    expect(useEditEpochStore.getState().bakeByDoc["d1"]).toBe(1);
  });

  // The idle backstop keys off this flag; if it stayed set after a bake the
  // backstop would re-fire forever (the original raw-vs-bake-comparison bug).
  it("the pending-bake flag is set by a soft edit and cleared by any bake", () => {
    const s = useEditEpochStore.getState();
    expect(useEditEpochStore.getState().pendingBake["d1"]).toBeUndefined();
    s.bumpEpochSoft("d1");
    expect(useEditEpochStore.getState().pendingBake["d1"]).toBe(true);
    // The idle backstop's bake clears it — so it fires exactly once.
    s.bumpBake("d1");
    expect(useEditEpochStore.getState().pendingBake["d1"]).toBeUndefined();
    // A hard edit clears it too (that reload bakes everything).
    s.bumpEpochSoft("d1");
    expect(useEditEpochStore.getState().pendingBake["d1"]).toBe(true);
    s.bumpEpoch("d1");
    expect(useEditEpochStore.getState().pendingBake["d1"]).toBeUndefined();
  });
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

  // A tab switch reuses the PdfViewer instance (only the id prop changes), so the
  // debounced value must snap to the new doc's epoch immediately — otherwise it
  // settles delayMs later and fires a second, spurious reload after every switch.
  it("adopts a switched-to document's epoch immediately (no cross-doc debounce)", () => {
    vi.useFakeTimers();
    // d1 has 2 edits; d2 is pristine.
    act(() => {
      useEditEpochStore.getState().bumpEpoch("d1");
      useEditEpochStore.getState().bumpEpoch("d1");
    });
    const { result, rerender } = renderHook(({ id }) => useDebouncedDocEpoch(id, 900), {
      initialProps: { id: "d1" },
    });
    expect(result.current).toBe(2);

    // Switch to d2 → the value is d2's epoch (0) at once, without advancing time.
    rerender({ id: "d2" });
    expect(result.current).toBe(0);

    // And switching back to d1 immediately reflects its epoch (still 2), no timer.
    rerender({ id: "d1" });
    expect(result.current).toBe(2);
  });
});
