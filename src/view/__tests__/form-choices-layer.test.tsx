// SPEC: P5-FORM-004 (P5.A4) — the choice overlay: renders a <select> per choice
// field with the field's options; changing the selection calls setChoiceField.
// IPC mocked; stores real.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

import type { ChoiceField } from "@/ipc/forms";

const { FIELDS } = vi.hoisted(() => ({
  FIELDS: [
    {
      name: "fruit",
      kind: "combo",
      rect: [72, 700, 250, 724],
      options: [
        { export: "Apple", label: "Apple" },
        { export: "chy", label: "Cherry" },
      ],
      selected: ["Apple"],
      multi: false,
    },
  ] satisfies ChoiceField[],
}));

vi.mock("@/ipc/forms", () => ({
  readChoiceFields: vi.fn().mockResolvedValue(FIELDS),
  setChoiceField: vi.fn().mockResolvedValue({ canUndo: true, canRedo: false }),
}));

import { readChoiceFields, setChoiceField } from "@/ipc/forms";
import { useFormStore } from "@/state/form-store";
import { FormChoicesLayer } from "@/view/form-choices-layer";

const DOC = "doc-1";
const mockSet = vi.mocked(setChoiceField);

const layer = () => (
  <FormChoicesLayer
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
  useFormStore.setState({ detected: { fieldCount: 1, hasXfa: false }, formMode: true });
  vi.mocked(readChoiceFields).mockResolvedValue(FIELDS);
});

describe("FormChoicesLayer", () => {
  it("renders a select prefilled with the current selection", async () => {
    render(layer());
    const select = (await screen.findByLabelText("Choice field fruit")) as HTMLSelectElement;
    expect(select.value).toBe("Apple");
    // Options include the labelled export/display pair.
    expect(screen.getByRole("option", { name: "Cherry" })).toBeTruthy();
  });

  it("writes the chosen export value on change", async () => {
    render(layer());
    const select = await screen.findByLabelText("Choice field fruit");
    fireEvent.change(select, { target: { value: "chy" } });
    await waitFor(() => expect(mockSet).toHaveBeenCalledWith(DOC, "fruit", ["chy"]));
  });

  it("renders nothing when not in form mode", () => {
    useFormStore.setState({ formMode: false });
    render(layer());
    expect(screen.queryByLabelText("Choice field fruit")).toBeNull();
  });
});
