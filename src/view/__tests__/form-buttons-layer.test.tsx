// SPEC: P5-FORM-003 (P5.A3) — the checkbox/radio overlay: renders a control per
// widget; clicking a checkbox toggles it, clicking a radio option selects it.
// IPC mocked; stores real.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

import type { ButtonField } from "@/ipc/forms";

const { BUTTONS } = vi.hoisted(() => ({
  BUTTONS: [
    { fieldName: "agree", kind: "checkbox", rect: [72, 700, 90, 718], onState: "Yes", checked: false },
    { fieldName: "color", kind: "radio", rect: [72, 660, 90, 678], onState: "Red", checked: false },
    { fieldName: "color", kind: "radio", rect: [72, 630, 90, 648], onState: "Green", checked: false },
  ] satisfies ButtonField[],
}));

vi.mock("@/ipc/forms", () => ({
  readButtonFields: vi.fn().mockResolvedValue(BUTTONS),
  setButtonField: vi.fn().mockResolvedValue({ canUndo: true, canRedo: false }),
}));

import { readButtonFields, setButtonField } from "@/ipc/forms";
import { useFormStore } from "@/state/form-store";
import { FormButtonsLayer } from "@/view/form-buttons-layer";

const DOC = "doc-1";
const mockSet = vi.mocked(setButtonField);

const layer = () => (
  <FormButtonsLayer
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
  useFormStore.setState({ detected: { fieldCount: 3, hasXfa: false }, formMode: true });
  vi.mocked(readButtonFields).mockResolvedValue(BUTTONS);
});

describe("FormButtonsLayer", () => {
  it("renders a control per widget", async () => {
    render(layer());
    expect(await screen.findByLabelText("checkbox agree Yes")).toBeTruthy();
    expect(screen.getByLabelText("radio color Red")).toBeTruthy();
    expect(screen.getByLabelText("radio color Green")).toBeTruthy();
  });

  it("toggles a checkbox on click (checked = !checked)", async () => {
    render(layer());
    const cb = await screen.findByLabelText("checkbox agree Yes");
    fireEvent.click(cb);
    await waitFor(() => expect(mockSet).toHaveBeenCalledWith(DOC, "agree", "Yes", true));
  });

  it("selects a radio option with checked = true", async () => {
    render(layer());
    const green = await screen.findByLabelText("radio color Green");
    fireEvent.click(green);
    await waitFor(() => expect(mockSet).toHaveBeenCalledWith(DOC, "color", "Green", true));
  });

  it("renders nothing when not in form mode", () => {
    useFormStore.setState({ formMode: false });
    render(layer());
    expect(screen.queryByLabelText("checkbox agree Yes")).toBeNull();
  });
});
