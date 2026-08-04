// SPEC: P5-FORM-002 (P5.A2) — the fill overlay: in form mode it lays an input over
// each text field, prefilled with /V and capped at /MaxLen; committing (blur)
// calls fillTextField. IPC + stores real except the IPC module, which is mocked.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

import type { FormField } from "@/ipc/forms";

const { FIELDS } = vi.hoisted(() => ({
  FIELDS: [
    { name: "first", rect: [72, 700, 300, 724], value: "Ada", maxLen: null, multiline: false },
    { name: "last", rect: [72, 660, 300, 684], value: "", maxLen: 5, multiline: false },
  ] satisfies FormField[],
}));

vi.mock("@/ipc/forms", () => ({
  readTextFields: vi.fn().mockResolvedValue(FIELDS),
  fillTextField: vi.fn().mockResolvedValue({ canUndo: true, canRedo: false }),
}));

import { fillTextField, readTextFields } from "@/ipc/forms";
import { useFormStore } from "@/state/form-store";
import { FormFieldsLayer } from "@/view/form-fields-layer";

const DOC = "doc-1";
const mockFill = vi.mocked(fillTextField);

const layer = () => (
  <FormFieldsLayer
    documentId={DOC}
    page={0}
    displayedWidth={612}
    displayedHeight={792}
    scale={1}
    rotation={0}
  />
);

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});
beforeEach(() => {
  useFormStore.setState({ detected: { fieldCount: 2, hasXfa: false }, formMode: true });
  vi.mocked(readTextFields).mockResolvedValue(FIELDS);
});

describe("FormFieldsLayer", () => {
  it("renders an input per field, prefilled with the value", async () => {
    render(layer());
    const first = (await screen.findByLabelText("Form field first")) as HTMLInputElement;
    expect(first.value).toBe("Ada");
    const last = screen.getByLabelText("Form field last") as HTMLInputElement;
    expect(last.value).toBe("");
    expect(last.maxLength).toBe(5);
  });

  it("commits a changed value through fillTextField on blur", async () => {
    render(layer());
    const last = (await screen.findByLabelText("Form field last")) as HTMLInputElement;
    fireEvent.change(last, { target: { value: "Byron" } });
    fireEvent.blur(last);
    await waitFor(() => expect(mockFill).toHaveBeenCalledWith(DOC, "last", "Byron"));
  });

  it("does not write when the value is unchanged", async () => {
    render(layer());
    const first = (await screen.findByLabelText("Form field first")) as HTMLInputElement;
    fireEvent.blur(first); // value still "Ada"
    expect(mockFill).not.toHaveBeenCalled();
  });

  it("renders nothing when not in form mode", () => {
    useFormStore.setState({ formMode: false });
    render(layer());
    expect(screen.queryByLabelText("Form field first")).toBeNull();
  });
});
