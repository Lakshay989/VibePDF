// SPEC: P2.A2 — the crash-recovery hook. Asserts it surfaces the entries
// from `recovery_list`, that "recover" opens the autosave file and drops
// the copy, and that "discard" drops the copy without opening.
//
// One test, one render: `use-recovery`'s once-per-launch scan is gated by
// a module-level flag (survives StrictMode), so a second renderHook in
// this file would not re-scan.

import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/recovery", () => ({
  recoveryList: vi.fn(),
  recoveryDiscard: vi.fn(),
}));

// Imported after the mock factory so these are the mocks.
import { recoveryDiscard, recoveryList } from "@/ipc/recovery";
import { useRecovery } from "@/app/use-recovery";

const mockList = vi.mocked(recoveryList);
const mockDiscard = vi.mocked(recoveryDiscard);

function entry(id: string) {
  return {
    documentId: id,
    originalPath: `/docs/${id}.pdf`,
    autosavePath: `/autosave/${id}.pdf`,
    savedAt: 1,
  };
}

describe("useRecovery", () => {
  it("surfaces entries, recovers (open + drop), and discards (drop only)", async () => {
    mockList.mockResolvedValue([entry("a"), entry("b")]);
    mockDiscard.mockResolvedValue();
    const openByPath = vi.fn().mockResolvedValue(undefined);

    const { result } = renderHook(() => useRecovery(openByPath));

    await waitFor(() => expect(result.current.entries).toHaveLength(2));

    // Recover "a": opens its autosave copy, drops it, removes from the list.
    await act(async () => {
      await result.current.recover(entry("a"));
    });
    expect(openByPath).toHaveBeenCalledWith("/autosave/a.pdf");
    expect(mockDiscard).toHaveBeenCalledWith("a");
    expect(result.current.entries.map((e) => e.documentId)).toEqual(["b"]);

    // Discard "b": drops the copy, never opens it.
    await act(async () => {
      await result.current.discard(entry("b"));
    });
    expect(mockDiscard).toHaveBeenCalledWith("b");
    expect(result.current.entries).toHaveLength(0);
    expect(openByPath).toHaveBeenCalledTimes(1); // discard did not open
  });
});
