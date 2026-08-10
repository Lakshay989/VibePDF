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
      multi: false, tooltip: null,
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

  // SPEC: P5-FORM-004 — P5 sweep B2. The single-select placeholder used to be a
  // real, selectable option, so picking it committed `[""]` and the backend
  // rejected it with "not an option".
  it("does not commit the empty placeholder option", async () => {
    render(layer());
    const select = (await screen.findByLabelText("Choice field fruit")) as HTMLSelectElement;
    const placeholder = Array.from(select.options).find((o) => o.value === "");
    expect(placeholder).toBeTruthy();
    expect(placeholder?.disabled).toBe(true);

    fireEvent.change(select, { target: { value: "" } });
    await waitFor(() => expect(mockSet).not.toHaveBeenCalled());
  });

  it("tells the user how to multi-select on a list box", async () => {
    vi.mocked(readChoiceFields).mockResolvedValue([
      { ...FIELDS[0]!, name: "tags", kind: "list", multi: true, selected: [] },
    ]);
    render(layer());
    const select = await screen.findByLabelText("Choice field tags");
    expect(select.getAttribute("title")).toMatch(/select more than one/i);
  });

  it("prefers the field tooltip over the multi-select hint", async () => {
    vi.mocked(readChoiceFields).mockResolvedValue([
      { ...FIELDS[0]!, multi: true, tooltip: "Pick your fruit" },
    ]);
    render(layer());
    const select = await screen.findByLabelText("Choice field fruit");
    expect(select.getAttribute("title")).toBe("Pick your fruit");
  });
});
