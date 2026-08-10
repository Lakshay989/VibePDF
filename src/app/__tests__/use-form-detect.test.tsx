// SPEC: P5-FORM-001 (P5.A1) — form detection populates the store that drives the
// "Form mode (N fields)" entry point.
//
// The re-read-on-epoch case is P5 sweep B3: creating or deleting a field changed
// the count, but ⌘Z did not — undo goes through the global history command, and
// nothing re-read the summary, so the header sat stale. IPC mocked; stores real.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";

vi.mock("@/ipc/forms", () => ({
  readFormSummary: vi.fn().mockResolvedValue({ fieldCount: 3, hasXfa: false }),
}));

import { readFormSummary } from "@/ipc/forms";
import { useFormDetect } from "@/app/use-form-detect";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useFormStore } from "@/state/form-store";

const DOC = "doc-1";
const mockRead = vi.mocked(readFormSummary);

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});
beforeEach(() => {
  useFormStore.setState({ detected: null, formMode: false });
  mockRead.mockResolvedValue({ fieldCount: 3, hasXfa: false });
});

describe("useFormDetect", () => {
  it("detects the form on open", async () => {
    renderHook(() => useFormDetect(DOC));
    await waitFor(() => expect(useFormStore.getState().detected?.fieldCount).toBe(3));
  });

  it("re-reads the summary when the document changes on disk (epoch bump)", async () => {
    renderHook(() => useFormDetect(DOC));
    await waitFor(() => expect(mockRead).toHaveBeenCalledTimes(1));

    // An undo that removes a created field: the count drops, and nothing but the
    // epoch tells the header about it.
    mockRead.mockResolvedValue({ fieldCount: 2, hasXfa: false });
    act(() => useEditEpochStore.getState().bumpEpoch(DOC));

    await waitFor(() => expect(useFormStore.getState().detected?.fieldCount).toBe(2));
  });

  it("stays in form mode across an edit", async () => {
    renderHook(() => useFormDetect(DOC));
    await waitFor(() => expect(mockRead).toHaveBeenCalled());
    useFormStore.getState().enterFormMode();

    act(() => useEditEpochStore.getState().bumpEpoch(DOC));
    await waitFor(() => expect(mockRead).toHaveBeenCalledTimes(2));
    // Filling a field must not kick the user out of the mode they're filling in.
    expect(useFormStore.getState().formMode).toBe(true);
  });

  it("clears detection and leaves form mode when the document changes", async () => {
    const { rerender } = renderHook(({ id }: { id: string }) => useFormDetect(id), {
      initialProps: { id: DOC },
    });
    await waitFor(() => expect(useFormStore.getState().detected).not.toBeNull());
    useFormStore.getState().enterFormMode();

    mockRead.mockResolvedValue({ fieldCount: 0, hasXfa: false });
    rerender({ id: "doc-2" });
    await waitFor(() => expect(useFormStore.getState().formMode).toBe(false));
  });

  it("survives a failed detection without throwing", async () => {
    mockRead.mockRejectedValue(new Error("nope"));
    renderHook(() => useFormDetect(DOC));
    await waitFor(() => expect(mockRead).toHaveBeenCalled());
    expect(useFormStore.getState().detected).toBeNull();
  });
});
