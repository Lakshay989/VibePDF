// SPEC: P2-SAVE-001 — the `savePdf` IPC wrapper. Thin glue, but the
// `path ?? null` marshalling matters: the Rust command distinguishes
// `None` (save to own path) from `Some(p)` (save-as), and JS `undefined`
// would not deserialize to `Option<String>` correctly.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

// Imported after the mock factory so this binding is the mock.
import { invoke } from "@/ipc/invoke";
import { savePdf } from "@/ipc/save";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
  mockInvoke.mockResolvedValue({ path: "/x.pdf", bytesWritten: 0, noOp: true });
});

describe("savePdf", () => {
  it("marshals an omitted path to null (same-path save)", async () => {
    await savePdf("doc-1");
    expect(mockInvoke).toHaveBeenCalledWith("pdf_save", {
      id: "doc-1",
      path: null,
    });
  });

  it("passes an explicit save-as path through", async () => {
    await savePdf("doc-1", "/out.pdf");
    expect(mockInvoke).toHaveBeenCalledWith("pdf_save", {
      id: "doc-1",
      path: "/out.pdf",
    });
  });

  it("returns the SaveOutcome from the backend", async () => {
    mockInvoke.mockResolvedValueOnce({
      path: "/out.pdf",
      bytesWritten: 1234,
      noOp: false,
    });
    const out = await savePdf("doc-1", "/out.pdf");
    expect(out).toEqual({ path: "/out.pdf", bytesWritten: 1234, noOp: false });
  });
});
