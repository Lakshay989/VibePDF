// SPEC: P4-EDIT-002 (P4.A2) — the readFontReport IPC wrapper marshals the
// document id to the Rust command and returns the typed report.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

import { invoke } from "@/ipc/invoke";
import { readFontReport, type FontReport } from "@/ipc/fonts";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
});

describe("readFontReport", () => {
  it("marshals the document id and returns the report", async () => {
    const report: FontReport = {
      needsFallback: true,
      fonts: [
        { fontName: "Helvetica", embedded: false, status: "standard", substitute: null },
        { fontName: "Calibri", embedded: false, status: "fallback", substitute: "Helvetica" },
      ],
    };
    mockInvoke.mockResolvedValue(report);

    const out = await readFontReport("doc-1");
    expect(mockInvoke).toHaveBeenCalledWith("pdf_read_font_report", { id: "doc-1" });
    expect(out).toEqual(report);
  });
});
