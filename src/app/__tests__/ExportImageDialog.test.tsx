// SPEC: P7-OCR-005 — the export-as-images dialog.
//
// What is worth pinning is what the user's choices *become* on the wire: the
// format, the DPI (the spec's range is the whole point of the field), the
// quality that only JPEG uses, and the pages list, where empty means the whole
// document and is easy to invert.
//
// Also pinned: an out-of-range DPI cannot be submitted. The backend refuses it
// too, but a user who has typed 1200 should be told before they pick a folder,
// not after.

import type { ComponentProps } from "react";

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

vi.mock("@/ipc/export-image", async () => {
  const actual = await vi.importActual<typeof import("@/ipc/export-image")>("@/ipc/export-image");
  return { ...actual, exportImages: vi.fn() };
});
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("@/app/report-error", () => ({ reportError: vi.fn() }));

import { ExportImageDialog } from "@/app/ExportImageDialog";
import { reportError } from "@/app/report-error";
import { exportImages, type ImageExportSummary } from "@/ipc/export-image";
import { open as openFolderDialog } from "@tauri-apps/plugin-dialog";

const mockExport = vi.mocked(exportImages);
const mockFolder = vi.mocked(openFolderDialog);
const mockReport = vi.mocked(reportError);

const summary = (over: Partial<ImageExportSummary> = {}): ImageExportSummary => ({
  pages: 5,
  files: ["doc-001.png", "doc-002.png", "doc-003.png", "doc-004.png", "doc-005.png"],
  bytes: 2_500_000,
  ...over,
});

const show = (props: Partial<ComponentProps<typeof ExportImageDialog>> = {}) =>
  render(
    <ExportImageDialog
      open
      documentId={"doc-1" as never}
      currentPage={2}
      pageCount={5}
      pageWidthPoints={612}
      pageHeightPoints={792}
      stem="report"
      onClose={props.onClose ?? vi.fn()}
      {...props}
    />,
  );

const submit = () => fireEvent.click(screen.getByRole("button", { name: /Choose folder/ }));
const dpiField = () => screen.getByRole("spinbutton");

beforeEach(() => {
  vi.clearAllMocks();
  mockFolder.mockResolvedValue("/tmp/out");
  mockExport.mockResolvedValue(summary());
});
afterEach(cleanup);

describe("ExportImageDialog", () => {
  it("shows nothing when closed", () => {
    show({ open: false });
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("offers all four formats the spec names", () => {
    show();
    for (const label of ["PNG", "JPG", "TIFF", "WebP"]) {
      // Anchored: the WebP description mentions PNG, so a loose match finds two.
      expect(screen.getByRole("radio", { name: new RegExp(`^${label}`) })).toBeTruthy();
    }
  });

  it("exports PNG at the default DPI over the whole document", async () => {
    show();
    submit();
    await waitFor(() => expect(mockExport).toHaveBeenCalled());
    expect(mockExport).toHaveBeenCalledWith("doc-1", "/tmp/out", "report", [], {
      format: "png",
      dpi: 150,
      quality: 85,
    });
  });

  it("sends the chosen format", async () => {
    show();
    fireEvent.click(screen.getByRole("radio", { name: /^TIFF/ }));
    submit();
    await waitFor(() => expect(mockExport).toHaveBeenCalled());
    expect(mockExport.mock.calls[0]?.[4]).toMatchObject({ format: "tiff" });
  });

  it("sends the typed DPI", async () => {
    show();
    fireEvent.change(dpiField(), { target: { value: "203" } });
    submit();
    await waitFor(() => expect(mockExport).toHaveBeenCalled());
    expect(mockExport.mock.calls[0]?.[4]).toMatchObject({ dpi: 203 });
  });

  it("states the pixel size the chosen DPI produces", () => {
    show();
    // 612 x 792 pt at 150 DPI is 1275 x 1650 px — the renderer's own sum.
    expect(screen.getByText(/1275 × 1650 pixels a page/)).toBeTruthy();
    fireEvent.change(dpiField(), { target: { value: "300" } });
    expect(screen.getByText(/2550 × 3300 pixels a page/)).toBeTruthy();
  });

  it("refuses a DPI outside the spec's range before a folder is picked", () => {
    show();
    fireEvent.change(dpiField(), { target: { value: "1200" } });
    expect(screen.getByRole("button", { name: /Choose folder/ })).toHaveProperty("disabled", true);
    fireEvent.change(dpiField(), { target: { value: "71" } });
    expect(screen.getByRole("button", { name: /Choose folder/ })).toHaveProperty("disabled", true);
    fireEvent.change(dpiField(), { target: { value: "72" } });
    expect(screen.getByRole("button", { name: /Choose folder/ })).toHaveProperty("disabled", false);
    expect(mockFolder).not.toHaveBeenCalled();
  });

  it("holds still when the DPI field is cleared", () => {
    // `valueAsNumber` on an empty number input is NaN, which would render as
    // "NaN × NaN pixels" and submit a DPI the backend cannot parse.
    show();
    fireEvent.change(dpiField(), { target: { value: "" } });
    expect(screen.getByRole("button", { name: /Choose folder/ })).toHaveProperty("disabled", true);
    expect(screen.getByText(/Enter a number between 72 and 600/)).toBeTruthy();
    // The field itself, not just the page text: a controlled input handed NaN
    // shows the string "NaN", which the user then has to delete by hand.
    expect((dpiField() as HTMLInputElement).value).toBe("");
  });

  it("offers quality only for JPEG, and sends what was picked", async () => {
    show();
    expect(screen.queryByRole("radio", { name: /Best quality/ })).toBeNull();
    fireEvent.click(screen.getByRole("radio", { name: /^JPG/ }));
    fireEvent.click(screen.getByRole("radio", { name: /Best quality/ }));
    submit();
    await waitFor(() => expect(mockExport).toHaveBeenCalled());
    expect(mockExport.mock.calls[0]?.[4]).toMatchObject({ format: "jpeg", quality: 95 });
  });

  it("exports only the current page when asked", async () => {
    show();
    fireEvent.click(screen.getByRole("radio", { name: /This page only/ }));
    submit();
    await waitFor(() => expect(mockExport).toHaveBeenCalled());
    expect(mockExport.mock.calls[0]?.[3]).toEqual([2]);
  });

  it("does not export when the folder dialog is cancelled", async () => {
    mockFolder.mockResolvedValue(null);
    show();
    submit();
    await waitFor(() => expect(mockFolder).toHaveBeenCalled());
    expect(mockExport).not.toHaveBeenCalled();
  });

  it("names the first file before the run, and reports what was written after", async () => {
    show();
    expect(screen.getByText(/report-001\.png onwards/)).toBeTruthy();
    submit();
    await waitFor(() => expect(screen.getByText(/Wrote 5 files/)).toBeTruthy());
    expect(screen.getByText(/2\.4 MB/)).toBeTruthy();
    expect(screen.getByText(/doc-001\.png through doc-005\.png/)).toBeTruthy();
  });

  it("names the right extension for JPEG before the run", () => {
    show();
    fireEvent.click(screen.getByRole("radio", { name: /^JPG/ }));
    expect(screen.getByText(/report-001\.jpg onwards/)).toBeTruthy();
  });

  it("reports a failure instead of claiming success", async () => {
    mockExport.mockRejectedValue(new Error("disk full"));
    show();
    submit();
    await waitFor(() => expect(mockReport).toHaveBeenCalled());
    expect(screen.queryByText(/Wrote/)).toBeNull();
  });
});
