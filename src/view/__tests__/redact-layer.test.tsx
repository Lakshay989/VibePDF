// SPEC: P6-SEC-010 (P6.D1c) — the redact overlay.
//
// The thing worth testing hardest is that nothing is removed without an
// explicit confirmation, and that the confirmation tells the truth about
// permanence. Every other edit in VibePDF is undoable forever; this one stops
// being undoable the moment the file is saved, and a user who does not know
// that will find out at the worst possible time.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";

vi.mock("@/ipc/pdf", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/ipc/pdf")>()),
  redactRegion: vi.fn(),
}));
vi.mock("@/app/report-error", () => ({ reportError: vi.fn() }));

import { reportError } from "@/app/report-error";
import { redactRegion } from "@/ipc/pdf";
import { useToolStore } from "@/state/tool-store";
import { RedactLayer } from "@/view/redact-layer";

const mockRedact = vi.mocked(redactRegion);
const mockReport = vi.mocked(reportError);

const layer = () => (
  <RedactLayer
    documentId="doc-1"
    page={0}
    displayedWidth={612}
    displayedHeight={792}
    scale={1}
    rotation={0}
  />
);

/** Drag from (x0,y0) to (x1,y1) on the overlay. */
function drag(x0: number, y0: number, x1: number, y1: number) {
  const el = screen.getByTestId("redact-layer");
  fireEvent.pointerDown(el, { clientX: x0, clientY: y0, pointerId: 1 });
  fireEvent.pointerMove(el, { clientX: x1, clientY: y1, pointerId: 1 });
  fireEvent.pointerUp(el, { clientX: x1, clientY: y1, pointerId: 1 });
}

beforeEach(() => {
  vi.clearAllMocks();
  mockRedact.mockResolvedValue({
    removed: 1,
    split: 0,
    removedWholeForSafety: 0,
    imagesRemoved: 0,
    history: { canUndo: true, canRedo: false, dirty: true },
  } as never);
  act(() => useToolStore.getState().setActiveTool("redact"));
});

afterEach(() => {
  act(() => useToolStore.getState().setActiveTool(null));
  cleanup();
});

describe("RedactLayer", () => {
  it("renders nothing when the tool is off", () => {
    act(() => useToolStore.getState().setActiveTool(null));
    const { container } = render(layer());
    expect(container.firstChild).toBeNull();
  });

  // The whole point of the confirmation step: dragging is not consent.
  it("does not redact on release — it asks first", () => {
    render(layer());
    drag(100, 100, 200, 140);

    expect(mockRedact).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog", { name: "Confirm redaction" })).toBeTruthy();
  });

  // "Are you sure?" tells a user nothing. What they need to know is when it
  // stops being undoable.
  it("says when the removal becomes permanent", () => {
    render(layer());
    drag(100, 100, 200, 140);

    const text = screen.getByRole("dialog").textContent ?? "";
    expect(text).toMatch(/deleted, not covered/i);
    expect(text).toMatch(/undo.*until you save/i);
    expect(text).toMatch(/gone for good/i);
  });

  it("redacts the dragged region on confirm", async () => {
    render(layer());
    drag(100, 100, 200, 140);
    await act(async () => {
      fireEvent.click(screen.getByText("Redact"));
    });

    expect(mockRedact).toHaveBeenCalledTimes(1);
    const [id, page, rect] = mockRedact.mock.calls[0] ?? [];
    expect(id).toBe("doc-1");
    expect(page).toBe(0);
    // PDF y grows upward, so the screen-space drag inverts: 792 - 140 = 652.
    expect(rect).toEqual([100, 652, 200, 692]);
  });

  it("removes nothing when cancelled", () => {
    render(layer());
    drag(100, 100, 200, 140);
    fireEvent.click(screen.getByText("Cancel"));

    expect(mockRedact).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  // A stray click while the tool is armed should not open a confirmation for a
  // region that would remove nothing.
  it("ignores a click that is not a drag", () => {
    render(layer());
    drag(100, 100, 101, 101);

    expect(screen.queryByRole("dialog")).toBeNull();
    expect(mockRedact).not.toHaveBeenCalled();
  });

  it("passes the metadata option only when ticked", async () => {
    render(layer());
    drag(100, 100, 200, 140);
    await act(async () => {
      fireEvent.click(screen.getByText("Redact"));
    });
    expect(mockRedact.mock.calls[0]?.[3]).toEqual({ removeMetadata: false });

    cleanup();
    vi.clearAllMocks();
    act(() => useToolStore.getState().setActiveTool("redact"));
    render(layer());
    drag(100, 100, 200, 140);
    fireEvent.click(screen.getByLabelText("Also remove document metadata"));
    await act(async () => {
      fireEvent.click(screen.getByText("Redact"));
    });
    expect(mockRedact.mock.calls[0]?.[3]).toEqual({ removeMetadata: true });
  });

  // A page whose text lives in a form is refused by the backend on purpose, so
  // the failure path here is a real outcome rather than an edge case.
  it("reports a refusal instead of failing silently", async () => {
    mockRedact.mockRejectedValue(new Error("This page draws text through a form"));
    render(layer());
    drag(100, 100, 200, 140);
    await act(async () => {
      fireEvent.click(screen.getByText("Redact"));
    });

    await vi.waitFor(() => expect(mockReport).toHaveBeenCalled());
  });
});
