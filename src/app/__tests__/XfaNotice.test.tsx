// SPEC: P5-FORM-005 (P5.A5) — the XFA-only notice shows only for an XFA layer
// with no fillable AcroForm fields, and its button converts (stripXfa). IPC
// mocked; the form store is real.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

vi.mock("@/ipc/forms", () => ({
  stripXfa: vi.fn().mockResolvedValue({ canUndo: true, canRedo: false }),
  readFormSummary: vi.fn().mockResolvedValue({ fieldCount: 0, hasXfa: false }),
}));

import { readFormSummary, stripXfa } from "@/ipc/forms";
import { useFormStore } from "@/state/form-store";
import { XfaNotice } from "@/app/XfaNotice";

const DOC = "doc-1";
const mockStrip = vi.mocked(stripXfa);

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});
beforeEach(() => {
  useFormStore.setState({ detected: { fieldCount: 0, hasXfa: true }, formMode: false });
  vi.mocked(readFormSummary).mockResolvedValue({ fieldCount: 0, hasXfa: false });
});

describe("XfaNotice", () => {
  it("shows for an XFA-only document", () => {
    render(<XfaNotice documentId={DOC} />);
    expect(screen.getByText(/XFA \(dynamic\) form/i)).toBeTruthy();
  });

  it("converts on click via stripXfa", async () => {
    render(<XfaNotice documentId={DOC} />);
    fireEvent.click(screen.getByLabelText("Convert XFA to a static read-only form"));
    await waitFor(() => expect(mockStrip).toHaveBeenCalledWith(DOC));
  });

  it("hides for a hybrid form (fillable AcroForm fields present)", () => {
    useFormStore.setState({ detected: { fieldCount: 3, hasXfa: true } });
    render(<XfaNotice documentId={DOC} />);
    expect(screen.queryByText(/XFA \(dynamic\) form/i)).toBeNull();
  });

  it("hides for a non-XFA document", () => {
    useFormStore.setState({ detected: { fieldCount: 0, hasXfa: false } });
    render(<XfaNotice documentId={DOC} />);
    expect(screen.queryByText(/XFA \(dynamic\) form/i)).toBeNull();
  });
});
