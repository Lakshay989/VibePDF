// SPEC: P6-SEC-012 (P6.D3) — the clean-document dialog.
//
// The checkbox sense is the thing worth pinning. Here a ticked box *removes*
// something, and in `ProtectDialog` a ticked box *grants* something; both are
// seven booleans in a modal, and an inversion in either direction produces a
// plausible-looking dialog that does the opposite of what it says. Nothing
// downstream can catch it — deleting all seven categories is as valid an
// operation as deleting none.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

vi.mock("@/ipc/pdf", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/ipc/pdf")>()),
  cleanDocument: vi.fn(),
}));
vi.mock("@/app/report-error", () => ({ reportError: vi.fn() }));

import { CleanDialog } from "@/app/CleanDialog";
import { reportError } from "@/app/report-error";
import { CLEAN_NOTHING, type CleanReport, cleanDocument } from "@/ipc/pdf";

const mockClean = vi.mocked(cleanDocument);
const mockReport = vi.mocked(reportError);

const LABELS = [
  "Document metadata",
  "Comments and markup",
  "Form data",
  "Bookmarks",
  "Attachments",
  "Embedded files",
  "Hidden text",
];

const emptyReport = (over: Partial<CleanReport> = {}): CleanReport => ({
  infoKeys: 0,
  xmpPackets: 0,
  hiddenTextRuns: 0,
  comments: 0,
  attachments: 0,
  bookmarks: 0,
  formFields: 0,
  embeddedFiles: 0,
  history: { canUndo: true, canRedo: false, dirty: true } as CleanReport["history"],
  ...over,
});

const onClose = vi.fn();
const dialog = () => <CleanDialog open documentId="doc-1" onClose={onClose} />;

beforeEach(() => {
  mockClean.mockResolvedValue(emptyReport({ infoKeys: 8, comments: 1 }));
  vi.clearAllMocks();
  mockClean.mockResolvedValue(emptyReport({ infoKeys: 8, comments: 1 }));
});
afterEach(cleanup);

describe("CleanDialog", () => {
  it("renders nothing when closed", () => {
    const { container } = render(
      <CleanDialog open={false} documentId="doc-1" onClose={onClose} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("offers all seven categories the spec names", () => {
    render(dialog());
    for (const label of LABELS) {
      expect(screen.getByLabelText(label)).toBeTruthy();
    }
  });

  // Every box deletes something, so the dialog must open inert.
  it("starts with nothing selected and the button disabled", () => {
    render(dialog());
    for (const label of LABELS) {
      expect((screen.getByLabelText(label) as HTMLInputElement).checked).toBe(false);
    }
    expect(screen.getByText("Clean").hasAttribute("disabled")).toBe(true);
  });

  it("sends exactly what was ticked and nothing else", async () => {
    render(dialog());
    fireEvent.click(screen.getByLabelText("Document metadata"));
    fireEvent.click(screen.getByLabelText("Bookmarks"));
    fireEvent.click(screen.getByText("Clean"));

    await waitFor(() => expect(mockClean).toHaveBeenCalled());
    expect(mockClean).toHaveBeenCalledWith("doc-1", {
      ...CLEAN_NOTHING,
      metadata: true,
      bookmarks: true,
    });
  });

  // Hidden text is the one destructive-by-surprise option: it is how a scanned
  // page is searchable, so a user who ticks it deserves to be told before, not
  // to discover it when search stops working.
  it("warns about scanned pages only when hidden text is selected", () => {
    render(dialog());
    expect(screen.queryByText(/un-searches any scanned page/i)).toBeNull();

    fireEvent.click(screen.getByLabelText("Hidden text"));
    expect(screen.getByText(/un-searches any scanned page/i)).toBeTruthy();

    fireEvent.click(screen.getByLabelText("Hidden text"));
    expect(screen.queryByText(/un-searches any scanned page/i)).toBeNull();
  });

  // The page looks identical after a clean, so the counts are the only
  // evidence the command did anything.
  it("reports what was removed", async () => {
    render(dialog());
    fireEvent.click(screen.getByLabelText("Document metadata"));
    fireEvent.click(screen.getByText("Clean"));

    await waitFor(() => expect(screen.getByText(/Removed:/)).toBeTruthy());
    expect(screen.getByText("8 metadata entries")).toBeTruthy();
    expect(screen.getByText("1 comments")).toBeTruthy();
    // Zero counts are noise, not information.
    expect(screen.queryByText(/0 bookmarks/)).toBeNull();
  });

  it("says so plainly when there was nothing to remove", async () => {
    mockClean.mockResolvedValue(emptyReport());
    render(dialog());
    fireEvent.click(screen.getByLabelText("Bookmarks"));
    fireEvent.click(screen.getByText("Clean"));

    await waitFor(() => expect(screen.getByText(/Nothing to remove/i)).toBeTruthy());
  });

  it("keeps the dialog open and reports when cleaning fails", async () => {
    mockClean.mockRejectedValue(new Error("actor is gone"));
    render(dialog());
    fireEvent.click(screen.getByLabelText("Bookmarks"));
    fireEvent.click(screen.getByText("Clean"));

    await waitFor(() => expect(mockReport).toHaveBeenCalled());
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByLabelText("Bookmarks")).toBeTruthy();
  });

  it("does not keep the selection after closing", async () => {
    render(dialog());
    fireEvent.click(screen.getByLabelText("Bookmarks"));
    fireEvent.click(screen.getByText("Cancel"));

    expect(onClose).toHaveBeenCalled();
    // Same mounted component, reopened: a stale tick would clean something the
    // user did not ask for this time round.
    await waitFor(() =>
      expect((screen.getByLabelText("Bookmarks") as HTMLInputElement).checked).toBe(false),
    );
  });
});
