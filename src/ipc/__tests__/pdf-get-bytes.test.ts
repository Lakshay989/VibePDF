// P4.HF28 — getPdfBytes decodes the raw-bytes IPC response.
//
// The Rust command returns bytes via `tauri::ipc::Response`, so `invoke`
// resolves to an `ArrayBuffer` rather than a JSON `number[]`. This guards the
// decode contract: on a large document the old array-of-numbers encoding
// ballooned the per-edit reload and edits silently failed to appear.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

import { getPdfBytes } from "@/ipc/pdf";
import { invoke } from "@/ipc/invoke";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
});

describe("getPdfBytes", () => {
  it("passes the document id to pdf_get_bytes", async () => {
    mockInvoke.mockResolvedValue(new Uint8Array([1, 2, 3]).buffer);
    await getPdfBytes("doc-1");
    expect(mockInvoke).toHaveBeenCalledWith("pdf_get_bytes", { id: "doc-1" });
  });

  it("decodes an ArrayBuffer response into the exact bytes (raw transport)", async () => {
    const source = new Uint8Array([0x25, 0x50, 0x44, 0x46, 0x00, 0xff, 0x80]); // "%PDF" + edges
    mockInvoke.mockResolvedValue(source.buffer);
    const out = await getPdfBytes("doc-1");
    expect(out).toBeInstanceOf(Uint8Array);
    expect(Array.from(out)).toEqual(Array.from(source));
  });

  it("stays correct if the transport ever yields a number[]", async () => {
    // `new Uint8Array(buf)` accepts array-likes too, so a fallback JSON
    // encoding would still decode losslessly.
    mockInvoke.mockResolvedValue([10, 20, 30] as unknown as ArrayBuffer);
    const out = await getPdfBytes("doc-1");
    expect(Array.from(out)).toEqual([10, 20, 30]);
  });
});
