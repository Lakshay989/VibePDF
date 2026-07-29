// SPEC: P4-EDIT-011 (P4.D4) — the page-numbers dialog: format/position/align +
// starting number + skip-pages, dispatched through the mocked IPC.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

vi.mock("@/ipc/page-numbers", () => ({
  addPageNumbers: vi.fn().mockResolvedValue({ canUndo: true, canRedo: false }),
}));

import { addPageNumbers } from "@/ipc/page-numbers";
import { PageNumbersDialog } from "@/app/PageNumbersDialog";

const DOC = "doc-1";
const mockAdd = vi.mocked(addPageNumbers);

const open = (pageCount = 10) =>
  render(
    <PageNumbersDialog open documentId={DOC} pageCount={pageCount} onClose={() => {}} />,
  );

beforeEach(() => {
  mockAdd.mockClear();
});
afterEach(() => cleanup());

describe("PageNumbersDialog", () => {
  it("offers every format option", () => {
    open();
    const format = screen.getByLabelText("Format") as HTMLSelectElement;
    const values = Array.from(format.options).map((o) => o.value);
    expect(values).toEqual([
      "decimal",
      "decimal-slash-total",
      "page-x-of-n",
      "lower-roman",
      "upper-roman",
      "lower-alpha",
      "upper-alpha",
    ]);
  });

  it("applies with the defaults (footer/center/decimal, start 1, no exclusions)", async () => {
    open();
    fireEvent.click(screen.getByText("Apply"));
    await waitFor(() => expect(mockAdd).toHaveBeenCalledTimes(1));
    expect(mockAdd).toHaveBeenCalledWith(
      DOC,
      [],
      expect.objectContaining({
        position: "footer",
        align: "center",
        format: "decimal",
        start: 1,
      }),
    );
  });

  it("parses skip-pages into 0-based excluded indices", async () => {
    open();
    fireEvent.change(screen.getByLabelText("Skip pages"), { target: { value: "2, 4" } });
    fireEvent.click(screen.getByText("Apply"));
    await waitFor(() => expect(mockAdd).toHaveBeenCalledTimes(1));
    expect(mockAdd.mock.calls[0][1]).toEqual([1, 3]);
  });

  it("passes the chosen format and start number", async () => {
    open();
    fireEvent.change(screen.getByLabelText("Format"), { target: { value: "lower-roman" } });
    fireEvent.change(screen.getByLabelText("Starting number"), { target: { value: "3" } });
    fireEvent.click(screen.getByText("Apply"));
    await waitFor(() => expect(mockAdd).toHaveBeenCalledTimes(1));
    expect(mockAdd.mock.calls[0][2]).toMatchObject({ format: "lower-roman", start: 3 });
  });

  it("rejects an empty/invalid starting number without calling the backend", async () => {
    // The input's min={1} lets the browser block a literal 0; the JS guard is the
    // backstop for a cleared field (empty passes native validation, fails ours).
    open();
    fireEvent.change(screen.getByLabelText("Starting number"), { target: { value: "" } });
    fireEvent.click(screen.getByText("Apply"));
    expect(await screen.findByRole("alert")).toBeTruthy();
    expect(mockAdd).not.toHaveBeenCalled();
  });

  it("surfaces an out-of-range skip page as an error", async () => {
    open(5);
    fireEvent.change(screen.getByLabelText("Skip pages"), { target: { value: "9" } });
    fireEvent.click(screen.getByText("Apply"));
    expect(await screen.findByRole("alert")).toBeTruthy();
    expect(mockAdd).not.toHaveBeenCalled();
  });
});
