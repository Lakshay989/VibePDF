// SPEC: P5-FORM-006 (P5.B1) — the create-text-field overlay: drag a box, fill the
// field config, confirm → addTextField. IPC mocked; tool store real.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

vi.mock("@/ipc/forms", () => ({
  addTextField: vi.fn().mockResolvedValue({ canUndo: true, canRedo: false }),
  readFormSummary: vi.fn().mockResolvedValue({ fieldCount: 1, hasXfa: false }),
}));

import { addTextField } from "@/ipc/forms";
import { useToolStore } from "@/state/tool-store";
import { FormCreateLayer } from "@/view/form-create-layer";

const DOC = "doc-1";
const mockAdd = vi.mocked(addTextField);

const layer = () => (
  <FormCreateLayer
    documentId={DOC}
    page={0}
    displayedWidth={612}
    displayedHeight={792}
    scale={1}
    rotation={0}
  />
);

/** Drag a box on the overlay to open the config popover. */
function dragBox(el: Element) {
  fireEvent.pointerDown(el, { clientX: 20, clientY: 20, pointerId: 1 });
  fireEvent.pointerMove(el, { clientX: 200, clientY: 60, pointerId: 1 });
  fireEvent.pointerUp(el, { clientX: 200, clientY: 60, pointerId: 1 });
}

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});
beforeEach(() => {
  useToolStore.setState({ activeTool: "create-text-field" });
});

describe("FormCreateLayer", () => {
  it("renders nothing unless the tool is active", () => {
    useToolStore.setState({ activeTool: null });
    const { container } = render(layer());
    expect(container.firstChild).toBeNull();
  });

  it("drag → config → create calls addTextField", async () => {
    const { container } = render(layer());
    dragBox(container.firstElementChild as Element);

    const name = await screen.findByLabelText("Field name");
    fireEvent.change(name, { target: { value: "email" } });
    fireEvent.click(screen.getByText("Create field"));

    await waitFor(() => expect(mockAdd).toHaveBeenCalledTimes(1));
    const [id, page, , field] = mockAdd.mock.calls[0]!;
    expect(id).toBe(DOC);
    expect(page).toBe(0);
    expect(field.name).toBe("email");
  });

  it("blocks create with an empty name", async () => {
    const { container } = render(layer());
    dragBox(container.firstElementChild as Element);
    await screen.findByLabelText("Field name");
    // Name left empty → the button is disabled.
    expect(screen.getByText("Create field").hasAttribute("disabled")).toBe(true);
  });
});
