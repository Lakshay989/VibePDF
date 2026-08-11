// P6.A1 — the store re-reads from the backend after every mutation.
//
// That is the point of it: the Rust side owns ordering and the cap, so a local
// patch could disagree with disk (e.g. after a prune). IPC mocked; store real.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/signatures", () => ({
  listSignatures: vi.fn(),
  addSignature: vi.fn(),
  removeSignature: vi.fn(),
}));

import { addSignature, listSignatures, removeSignature } from "@/ipc/signatures";
import { useSignatureStore } from "@/state/signature-store";

const mockList = vi.mocked(listSignatures);
const mockAdd = vi.mocked(addSignature);
const mockRemove = vi.mocked(removeSignature);

const entry = (id: string, createdAt: number) =>
  ({ id, kind: "draw" as const, createdAt });

beforeEach(() => {
  useSignatureStore.setState({ entries: [], loading: false });
  vi.clearAllMocks();
});
afterEach(() => vi.clearAllMocks());

describe("useSignatureStore", () => {
  it("loads the library on refresh", async () => {
    mockList.mockResolvedValue([entry("b", 2), entry("a", 1)]);
    await useSignatureStore.getState().refresh();

    expect(useSignatureStore.getState().entries.map((e) => e.id)).toEqual(["b", "a"]);
    expect(useSignatureStore.getState().loading).toBe(false);
  });

  it("re-reads after an add rather than patching locally", async () => {
    // The backend may prune on add, so its post-write list is the truth.
    mockAdd.mockResolvedValue(entry("new", 9));
    mockList.mockResolvedValue([entry("new", 9)]);

    const added = await useSignatureStore.getState().add("draw", Uint8Array.from([1]));

    expect(added.id).toBe("new");
    expect(mockList).toHaveBeenCalledTimes(1);
    expect(useSignatureStore.getState().entries.map((e) => e.id)).toEqual(["new"]);
  });

  it("re-reads after a remove", async () => {
    useSignatureStore.setState({ entries: [entry("a", 1), entry("b", 2)] });
    mockRemove.mockResolvedValue(undefined);
    mockList.mockResolvedValue([entry("b", 2)]);

    await useSignatureStore.getState().remove("a");

    expect(mockRemove).toHaveBeenCalledWith("a");
    expect(useSignatureStore.getState().entries.map((e) => e.id)).toEqual(["b"]);
  });

  it("keeps the previous list when a refresh fails", async () => {
    useSignatureStore.setState({ entries: [entry("a", 1)] });
    mockList.mockRejectedValue(new Error("disk gone"));

    await expect(useSignatureStore.getState().refresh()).rejects.toThrow("disk gone");
    // Blanking the picker on a transient read failure would be worse than stale.
    expect(useSignatureStore.getState().entries.map((e) => e.id)).toEqual(["a"]);
    expect(useSignatureStore.getState().loading).toBe(false);
  });
});
