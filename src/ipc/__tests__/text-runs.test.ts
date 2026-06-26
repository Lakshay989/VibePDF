// SPEC: P4-EDIT-001 (P4.A1) — the extractTextRuns IPC wrapper marshals (id, page)
// to the Rust command and returns the typed run array.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

import { invoke } from "@/ipc/invoke";
import { extractTextRuns, type TextRun } from "@/ipc/text-runs";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
});

describe("extractTextRuns", () => {
  it("marshals the document id + page and returns the runs", async () => {
    const runs: TextRun[] = [
      {
        text: "Hello",
        bbox: [72, 700, 140, 712],
        fontName: "Helvetica",
        embedded: false,
        fontSize: 12,
        color: "#000000",
        transform: [1, 0, 0, 1, 72, 700],
      },
    ];
    mockInvoke.mockResolvedValue(runs);

    const out = await extractTextRuns("doc-1", 0);
    expect(mockInvoke).toHaveBeenCalledWith("pdf_extract_text_runs", { id: "doc-1", page: 0 });
    expect(out).toEqual(runs);
  });
});
