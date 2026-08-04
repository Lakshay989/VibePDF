// SPEC: P5-FORM-001 (P5.A1) — the readFormSummary IPC wrapper marshals the
// document id to the Rust command and returns the typed summary.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/ipc/invoke", () => ({ invoke: vi.fn() }));

import { invoke } from "@/ipc/invoke";
import { readFormSummary, type FormSummary } from "@/ipc/forms";

const mockInvoke = vi.mocked(invoke);

beforeEach(() => {
  mockInvoke.mockReset();
});

describe("readFormSummary", () => {
  it("marshals the document id and returns the summary", async () => {
    const summary: FormSummary = { fieldCount: 3, hasXfa: false };
    mockInvoke.mockResolvedValue(summary);

    const out = await readFormSummary("doc-1");
    expect(mockInvoke).toHaveBeenCalledWith("pdf_read_form_summary", { id: "doc-1" });
    expect(out).toEqual(summary);
  });

  it("carries the XFA flag through", async () => {
    mockInvoke.mockResolvedValue({ fieldCount: 0, hasXfa: true } satisfies FormSummary);
    const out = await readFormSummary("doc-2");
    expect(out.hasXfa).toBe(true);
    expect(out.fieldCount).toBe(0);
  });
});
