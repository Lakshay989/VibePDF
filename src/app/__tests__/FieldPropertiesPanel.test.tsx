// SPEC: P5-FORM-006b/006c (P5.B3) — the panel lists a page's fields in tab
// order, edits the selected field's properties, reorders the tab sequence, and
// deletes a field. IPC mocked; stores real.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

import type { PageField } from "@/ipc/forms";

const { FIELDS } = vi.hoisted(() => ({
  FIELDS: [
    { name: "a", kind: "text", rect: [72, 700, 200, 724] },
    { name: "b", kind: "checkbox", rect: [72, 660, 90, 678] },
  ] satisfies PageField[],
}));

vi.mock("@/ipc/forms", () => ({
  readPageFields: vi.fn().mockResolvedValue(FIELDS),
  readFormSummary: vi.fn().mockResolvedValue({ fieldCount: 2, hasXfa: false }),
  updateFieldProperties: vi.fn().mockResolvedValue({ canUndo: true, canRedo: false }),
  setTabOrder: vi.fn().mockResolvedValue({ canUndo: true, canRedo: false }),
  deleteField: vi.fn().mockResolvedValue({ canUndo: true, canRedo: false }),
}));

import { deleteField, readPageFields, setTabOrder, updateFieldProperties } from "@/ipc/forms";
import { useFormStore } from "@/state/form-store";
import { FieldPropertiesPanel } from "@/app/FieldPropertiesPanel";

const DOC = "doc-1";
const mockUpdate = vi.mocked(updateFieldProperties);
const mockOrder = vi.mocked(setTabOrder);
const mockDelete = vi.mocked(deleteField);

const panel = () => <FieldPropertiesPanel documentId={DOC} page={0} />;

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});
beforeEach(() => {
  useFormStore.setState({ detected: { fieldCount: 2, hasXfa: false }, formMode: true });
  vi.mocked(readPageFields).mockResolvedValue(FIELDS);
});

describe("FieldPropertiesPanel", () => {
  it("lists the page's fields in tab order", async () => {
    render(panel());
    expect(await screen.findByLabelText("Field a")).toBeTruthy();
    expect(screen.getByLabelText("Field b")).toBeTruthy();
  });

  it("renders nothing when not in form mode", () => {
    useFormStore.setState({ formMode: false });
    const { container } = render(panel());
    expect(container.firstChild).toBeNull();
  });

  it("applies edited properties to the selected field", async () => {
    render(panel());
    fireEvent.click(await screen.findByLabelText("Field a"));
    fireEvent.change(screen.getByLabelText("Field name"), { target: { value: "first" } });
    fireEvent.change(screen.getByLabelText("Tooltip"), { target: { value: "Your name" } });
    fireEvent.click(screen.getByText("Apply"));

    await waitFor(() => expect(mockUpdate).toHaveBeenCalledTimes(1));
    const [id, target, patch] = mockUpdate.mock.calls[0]!;
    expect(id).toBe(DOC);
    expect(target).toBe("a");
    expect(patch.newName).toBe("first");
    expect(patch.tooltip).toBe("Your name");
  });

  it("reorders the tab sequence", async () => {
    render(panel());
    fireEvent.click(await screen.findByLabelText("Move b earlier"));
    await waitFor(() => expect(mockOrder).toHaveBeenCalledWith(DOC, 0, ["b", "a"]));
  });

  it("deletes the selected field", async () => {
    render(panel());
    fireEvent.click(await screen.findByLabelText("Field b"));
    fireEvent.click(screen.getByLabelText("Delete field b"));
    await waitFor(() => expect(mockDelete).toHaveBeenCalledWith(DOC, "b"));
  });
});
