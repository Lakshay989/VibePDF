// SPEC: P2-PAGE-005 (support) — the peekPageCount IPC wrapper marshals the
// path and returns the page count.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

import { peekPageCount } from "@/ipc/peek";
import { invoke } from "@/ipc/invoke";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
  mockInvoke.mockResolvedValue(3);
});

describe("peekPageCount", () => {
  it("marshals the path", async () => {
    await peekPageCount("/b.pdf");
    expect(mockInvoke).toHaveBeenCalledWith("pdf_peek_page_count", { path: "/b.pdf" });
  });

  it("returns the count from the backend", async () => {
    mockInvoke.mockResolvedValueOnce(7);
    expect(await peekPageCount("/b.pdf")).toBe(7);
  });
});
