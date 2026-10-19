// SPEC: P7-OCR-010 — the compress dialog.
//
// What is pinned: the level reaches the backend, the result is reported as a
// real before/after rather than a claim, and the "nothing to do" case is said
// plainly instead of being dressed up as a saving.
//
// Also pinned: it saves a copy. The whole safety story of a lossy operation is
// that the original survives, so a dialog that ever wrote in place would be a
// defect, not a preference.

import type { ComponentProps } from "react";

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

vi.mock("@/ipc/compress", () => ({ compressDocument: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn() }));
vi.mock("@/app/report-error", () => ({ reportError: vi.fn() }));

import { CompressDialog } from "@/app/CompressDialog";
import { reportError } from "@/app/report-error";
import { type CompressReport, compressDocument } from "@/ipc/compress";
import { save as saveFileDialog } from "@tauri-apps/plugin-dialog";

const mockCompress = vi.mocked(compressDocument);
const mockSave = vi.mocked(saveFileDialog);
const mockReport = vi.mocked(reportError);

const report = (over: Partial<CompressReport> = {}): CompressReport => ({
  beforeBytes: 28_000_000,
  afterBytes: 1_800_000,
  imagesRecompressed: 6,
  imagesUntouched: 0,
  streamsDeflated: 0,
  ...over,
});

const show = (props: Partial<ComponentProps<typeof CompressDialog>> = {}) =>
  render(
    <CompressDialog
      open
      documentId={"doc-1" as never}
      suggestedName="report-compressed.pdf"
      currentBytes={28_000_000}
      pageCount={6}
      onClose={props.onClose ?? vi.fn()}
      {...props}
    />,
  );

const submit = () => fireEvent.click(screen.getByRole("button", { name: /Save a copy/ }));

beforeEach(() => {
  vi.clearAllMocks();
  mockSave.mockResolvedValue("/tmp/out.pdf");
  mockCompress.mockResolvedValue(report());
});
afterEach(cleanup);

describe("CompressDialog", () => {
  it("shows nothing when closed", () => {
    show({ open: false });
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("offers the three levels the spec names", () => {
    show();
    expect(screen.getAllByRole("radio")).toHaveLength(3);
  });

  it("says the open document is not changed", () => {
    // The safety promise of a lossy operation, in the dialog rather than only
    // in a comment.
    show();
    expect(screen.getByText(/is not changed/)).toBeTruthy();
  });

  it("compresses at medium by default", async () => {
    show();
    submit();
    await waitFor(() => expect(mockCompress).toHaveBeenCalled());
    expect(mockCompress).toHaveBeenCalledWith("doc-1", "/tmp/out.pdf", "medium");
  });

  it("sends the chosen level", async () => {
    show();
    fireEvent.click(screen.getByRole("radio", { name: /Smallest/ }));
    submit();
    await waitFor(() => expect(mockCompress).toHaveBeenCalled());
    expect(mockCompress.mock.calls[0]?.[2]).toBe("high");
  });

  it("suggests a name that will not overwrite the original", async () => {
    show();
    submit();
    await waitFor(() => expect(mockSave).toHaveBeenCalled());
    expect(mockSave.mock.calls[0]?.[0]).toMatchObject({
      defaultPath: "report-compressed.pdf",
    });
  });

  it("reports the real before and after", async () => {
    show();
    submit();
    await waitFor(() => expect(screen.getByText(/26\.7 MB/)).toBeTruthy());
    expect(screen.getByText(/1\.7 MB/)).toBeTruthy();
    expect(screen.getByText(/6 images recompressed/)).toBeTruthy();
  });

  it("says plainly when there was nothing to save", async () => {
    // A file that cannot shrink must not be reported as a 100% saving or as a
    // success with no number — the user needs to know nothing was degraded.
    mockCompress.mockResolvedValue(
      report({ beforeBytes: 5893, afterBytes: 5893, imagesRecompressed: 0, imagesUntouched: 1 }),
    );
    show();
    submit();
    await waitFor(() => expect(screen.getByText(/already as small as we can make it/)).toBeTruthy());
    expect(screen.queryByText(/saved/)).toBeNull();
  });

  it("estimates the result before running, and scales it with the level", () => {
    show();
    // Medium is the default. Sizes are binary: 28,000,000 bytes is 26.7 MB.
    expect(screen.getByText(/26\.7 MB now/)).toBeTruthy();
    const medium = screen.getByText(/roughly/).textContent ?? "";
    fireEvent.click(screen.getByRole("radio", { name: /Smallest/ }));
    const high = screen.getByText(/roughly/).textContent ?? "";
    expect(high).not.toBe(medium);
  });

  it("omits the estimate when the file size is unknown", () => {
    show({ currentBytes: 0 });
    expect(screen.queryByText(/0 bytes now/)).toBeNull();
    expect(screen.getByText(/About 2 seconds for 6 pages/)).toBeTruthy();
  });

  it("does not compress when the save dialog is cancelled", async () => {
    mockSave.mockResolvedValue(null);
    show();
    submit();
    await waitFor(() => expect(mockSave).toHaveBeenCalled());
    expect(mockCompress).not.toHaveBeenCalled();
  });

  it("reports a failure instead of claiming success", async () => {
    mockCompress.mockRejectedValue(new Error("disk full"));
    show();
    submit();
    await waitFor(() => expect(mockReport).toHaveBeenCalled());
    expect(screen.queryByText(/saved/)).toBeNull();
  });
});
