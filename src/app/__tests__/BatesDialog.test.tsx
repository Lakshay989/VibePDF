// SPEC: P4-EDIT-012 (P4.D5) — the Bates dialog: prefix/suffix/padding/start +
// position/align, dispatched through the mocked IPC, with a live preview.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

vi.mock("@/ipc/bates", () => ({
  addBates: vi.fn().mockResolvedValue({ canUndo: true, canRedo: false }),
}));

import { addBates } from "@/ipc/bates";
import { BatesDialog } from "@/app/BatesDialog";

const DOC = "doc-1";
const mockAdd = vi.mocked(addBates);

const open = (pageCount = 10) =>
  render(<BatesDialog open documentId={DOC} pageCount={pageCount} onClose={() => {}} />);

beforeEach(() => mockAdd.mockClear());
afterEach(() => cleanup());

describe("BatesDialog", () => {
  it("applies with footer/right defaults and the entered prefix/padding/start", async () => {
    open();
    fireEvent.change(screen.getByLabelText("Prefix"), { target: { value: "ABC" } });
    fireEvent.click(screen.getByText("Apply"));
    await waitFor(() => expect(mockAdd).toHaveBeenCalledTimes(1));
    expect(mockAdd).toHaveBeenCalledWith(
      DOC,
      expect.objectContaining({
        position: "footer",
        align: "right",
        prefix: "ABC",
        suffix: "",
        padding: 6,
        start: 1,
      }),
    );
  });

  it("shows a live preview that reflects prefix, padding, and start", () => {
    open();
    fireEvent.change(screen.getByLabelText("Prefix"), { target: { value: "ABC" } });
    fireEvent.change(screen.getByLabelText("Padding digits"), { target: { value: "6" } });
    fireEvent.change(screen.getByLabelText("Starting number"), { target: { value: "1" } });
    // 10-page doc, start 1 → first ABC000001 … last ABC000010.
    expect(screen.getByText("ABC000001")).toBeTruthy();
    expect(screen.getByText("ABC000010")).toBeTruthy();
  });

  it("rejects an empty starting number without calling the backend", async () => {
    open();
    fireEvent.change(screen.getByLabelText("Starting number"), { target: { value: "" } });
    fireEvent.click(screen.getByText("Apply"));
    expect(await screen.findByRole("alert")).toBeTruthy();
    expect(mockAdd).not.toHaveBeenCalled();
  });

  it("rejects an empty padding without calling the backend", async () => {
    open();
    fireEvent.change(screen.getByLabelText("Padding digits"), { target: { value: "" } });
    fireEvent.click(screen.getByText("Apply"));
    expect(await screen.findByRole("alert")).toBeTruthy();
    expect(mockAdd).not.toHaveBeenCalled();
  });

  it("passes suffix and a chosen alignment", async () => {
    open();
    fireEvent.change(screen.getByLabelText("Suffix"), { target: { value: "-EX" } });
    fireEvent.change(screen.getByLabelText("Alignment"), { target: { value: "center" } });
    fireEvent.click(screen.getByText("Apply"));
    await waitFor(() => expect(mockAdd).toHaveBeenCalledTimes(1));
    expect(mockAdd.mock.calls[0][1]).toMatchObject({ suffix: "-EX", align: "center" });
  });
});
