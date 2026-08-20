// SPEC: P6-SEC-011 (P6.D2b) — find, review, apply.
//
// The requirement is that the user confirms *before* anything is applied, so
// most of these tests are about the list being reviewable rather than about it
// being complete: nothing pre-selected, the matched text visible, and pages
// that could not be searched shown as gaps rather than omitted.
//
// The last of those is the one that matters most. A document reported clean
// because half of it was unreadable looks exactly like a document that is
// clean, and only the dialog can tell the difference.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

vi.mock("@/ipc/pdf", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/ipc/pdf")>()),
  findRedactionMatches: vi.fn(),
  redactRegion: vi.fn(),
}));
vi.mock("@/app/report-error", () => ({ reportError: vi.fn() }));

import { FindRedactDialog } from "@/app/FindRedactDialog";
import { reportError } from "@/app/report-error";
import { findRedactionMatches, type MatchHit, redactRegion } from "@/ipc/pdf";

const mockFind = vi.mocked(findRedactionMatches);
const mockRedact = vi.mocked(redactRegion);
const mockReport = vi.mocked(reportError);

const hit = (over: Partial<MatchHit> = {}): MatchHit => ({
  page: 0,
  rect: [100, 690, 170, 710],
  kind: "ssn",
  preview: "123-45-6789",
  coversWholeRun: false,
  unreadable: false,
  ...over,
});

const onClose = vi.fn();
const dialog = () => <FindRedactDialog open documentId="doc-1" onClose={onClose} />;

/** Click Find and wait for the list. */
const search = async () => {
  await act(async () => {
    fireEvent.click(screen.getByText("Find"));
  });
};

beforeEach(() => {
  vi.clearAllMocks();
  mockFind.mockResolvedValue([hit()]);
  mockRedact.mockResolvedValue({
    removed: 1,
    split: 1,
    removedWholeForSafety: 0,
    imagesRemoved: 0,
    history: { canUndo: true, canRedo: false, dirty: true },
  } as never);
});
afterEach(cleanup);

describe("FindRedactDialog", () => {
  it("renders nothing when closed", () => {
    const { container } = render(
      <FindRedactDialog open={false} documentId="doc-1" onClose={onClose} />,
    );
    expect(container.firstChild).toBeNull();
  });

  // SPEC: P6-SEC-011 — searching must not redact. If finding applied anything,
  // the confirm step the requirement is built around would not exist.
  it("searching removes nothing", async () => {
    render(dialog());
    await search();

    expect(mockFind).toHaveBeenCalled();
    expect(mockRedact).not.toHaveBeenCalled();
  });

  // A list that arrives pre-confirmed is a list nobody reads.
  it("selects nothing by default", async () => {
    render(dialog());
    await search();

    const box = screen.getByLabelText("Redact 123-45-6789 on page 1") as HTMLInputElement;
    expect(box.checked).toBe(false);
    expect(screen.getByText(/Redact 0 selected/)).toBeTruthy();
  });

  // A confirm list that will not show what it found cannot be reviewed.
  it("shows the matched text and where it is", async () => {
    render(dialog());
    await search();

    expect(screen.getByText("123-45-6789")).toBeTruthy();
    expect(screen.getByText(/Social security number, page 1/)).toBeTruthy();
  });

  it("redacts only what was ticked", async () => {
    mockFind.mockResolvedValue([hit(), hit({ preview: "999-88-7777", page: 2 })]);
    render(dialog());
    await search();

    fireEvent.click(screen.getByLabelText("Redact 999-88-7777 on page 3"));
    await act(async () => {
      fireEvent.click(screen.getByText("Redact 1 selected"));
    });

    await waitFor(() => expect(mockRedact).toHaveBeenCalledTimes(1));
    expect(mockRedact.mock.calls[0]?.[1]).toBe(2);
  });

  // The failure this whole feature would otherwise invite: a document called
  // clean because part of it could not be looked at.
  it("says which pages could not be searched", async () => {
    mockFind.mockResolvedValue([hit(), hit({ page: 1, unreadable: true, preview: "" })]);
    render(dialog());
    await search();

    expect(screen.getByText(/could not be searched/i)).toBeTruthy();
    expect(screen.getByText(/page 2/)).toBeTruthy();
    // …and the gap is not offered as something to redact.
    expect(screen.getByText("1 match")).toBeTruthy();
  });

  // Over-removal is the deliberate choice when a font cannot be measured, but
  // the user confirming it should know they are agreeing to lose the line.
  it("warns when confirming will take the whole line", async () => {
    mockFind.mockResolvedValue([hit({ coversWholeRun: true })]);
    render(dialog());
    await search();

    expect(screen.getByText(/whole line goes, not just the match/i)).toBeTruthy();
  });

  it("says plainly when nothing matched", async () => {
    mockFind.mockResolvedValue([]);
    render(dialog());
    await search();

    expect(screen.getByText("Nothing matched.")).toBeTruthy();
  });

  it("reports a search failure instead of showing an empty list", async () => {
    mockFind.mockRejectedValue(new Error("That pattern isn't valid"));
    render(dialog());
    await search();

    expect(mockReport).toHaveBeenCalled();
    expect(screen.queryByText("Nothing matched.")).toBeNull();
  });

  it("does not keep the results after closing", async () => {
    render(dialog());
    await search();
    fireEvent.click(screen.getByText("Cancel"));

    expect(onClose).toHaveBeenCalled();
    expect(screen.queryByText("123-45-6789")).toBeNull();
  });
});
