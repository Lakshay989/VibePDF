// P6.A1 — the signature-library IPC wrappers marshal the right shapes.
//
// The interesting part is bytes: Tauri carries `Vec<u8>` as a JSON number
// array, so the wrapper converts in both directions and callers only ever see
// a `Uint8Array`.

import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

import { invoke } from "@/ipc/invoke";
import {
  addSignature,
  listSignatures,
  removeSignature,
  signatureBytes,
} from "@/ipc/signatures";

const mockInvoke = vi.mocked(invoke);

afterEach(() => vi.clearAllMocks());

describe("signature library IPC", () => {
  it("lists entries", async () => {
    const entry = { id: "a", kind: "draw" as const, createdAt: 1 };
    mockInvoke.mockResolvedValue([entry]);
    await expect(listSignatures()).resolves.toEqual([entry]);
    expect(mockInvoke).toHaveBeenCalledWith("signatures_list");
  });

  it("sends bytes as a plain number array", async () => {
    mockInvoke.mockResolvedValue({ id: "a", kind: "draw", createdAt: 1 });
    await addSignature("draw", Uint8Array.from([0x89, 0x50, 0x4e, 0x47]));

    const [, args] = mockInvoke.mock.calls[0]!;
    expect(args).toEqual({ kind: "draw", bytes: [0x89, 0x50, 0x4e, 0x47] });
    // A Uint8Array would serialise as an object with numeric keys, not an array.
    expect(Array.isArray((args as { bytes: unknown }).bytes)).toBe(true);
  });

  it("returns bytes as a Uint8Array", async () => {
    mockInvoke.mockResolvedValue([1, 2, 3]);
    const bytes = await signatureBytes("a");
    expect(bytes).toBeInstanceOf(Uint8Array);
    expect(Array.from(bytes)).toEqual([1, 2, 3]);
  });

  it("removes by id", async () => {
    mockInvoke.mockResolvedValue(undefined);
    await removeSignature("a");
    expect(mockInvoke).toHaveBeenCalledWith("signatures_remove", { id: "a" });
  });
});
