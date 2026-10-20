// SPEC: P7-OCR-009 (P7.C1a–c) — the create-PDF dialog.
//
// What is pinned: files reach the backend in the order shown, after any
// reordering; the chosen page size reaches it too; a cancelled save dialog
// creates nothing; warnings surface as toasts instead of vanishing; and a
// failure is reported rather than opening a file that was never written.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

vi.mock("@/ipc/create", () => ({ createPdfFromFiles: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }));
vi.mock("@/app/report-error", () => ({ reportError: vi.fn() }));

import { CreatePdfDialog, suggestedName } from "@/app/CreatePdfDialog";
import { reportError } from "@/app/report-error";
import { createPdfFromFiles } from "@/ipc/create";
import { useToastStore } from "@/state/toast-store";
import { open as openFileDialog, save as saveFileDialog } from "@tauri-apps/plugin-dialog";

const mockCreate = vi.mocked(createPdfFromFiles);
const mockOpen = vi.mocked(openFileDialog);
const mockSave = vi.mocked(saveFileDialog);
const mockReport = vi.mocked(reportError);

const summary = { pages: 2, images: 1, textFiles: 1, documents: 0, bytes: 1234, warnings: [] as string[] };

function show() {
  const onCreated = vi.fn();
  const onClose = vi.fn();
  render(<CreatePdfDialog open onClose={onClose} onCreated={onCreated} />);
  return { onCreated, onClose };
}

async function addFiles(paths: string[]) {
  mockOpen.mockResolvedValueOnce(paths);
  fireEvent.click(screen.getByText("Add files…"));
  await waitFor(() => expect(screen.getByText(paths[paths.length - 1]?.split("/").pop() ?? "")).toBeTruthy());
}

beforeEach(() => {
  mockCreate.mockResolvedValue(summary);
  mockSave.mockResolvedValue("/out/holiday.pdf");
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
  useToastStore.setState({ toasts: [] });
});

describe("CreatePdfDialog", () => {
  it("can't create anything until there are files", async () => {
    show();
    const button = screen.getByRole("button", { name: "Create…" });
    expect(button).toHaveProperty("disabled", true);
    await addFiles(["/in/holiday.jpg"]);
    expect(button).toHaveProperty("disabled", false);
  });

  it("sends the files in the order shown, after reordering and removing", async () => {
    show();
    await addFiles(["/in/a.jpg", "/in/b.txt", "/in/c.png"]);
    fireEvent.click(screen.getByLabelText("Move c.png up"));
    fireEvent.click(screen.getByLabelText("Remove a.jpg"));
    fireEvent.click(screen.getByRole("button", { name: "Create…" }));
    await waitFor(() => expect(mockCreate).toHaveBeenCalled());
    expect(mockCreate.mock.calls[0]?.[0]).toEqual(["/in/c.png", "/in/b.txt"]);
  });

  it("sends the page size and opens what it made", async () => {
    const { onCreated } = show();
    await addFiles(["/in/holiday.jpg"]);
    fireEvent.change(screen.getByLabelText("Page size"), { target: { value: "letter" } });
    fireEvent.click(screen.getByRole("button", { name: "Create…" }));
    await waitFor(() => expect(onCreated).toHaveBeenCalledWith("/out/holiday.pdf"));
    expect(mockCreate).toHaveBeenCalledWith(["/in/holiday.jpg"], "/out/holiday.pdf", "letter", false);
    expect(mockSave.mock.calls[0]?.[0]).toMatchObject({ defaultPath: "holiday.pdf" });
  });

  it("creates nothing when the save dialog is cancelled", async () => {
    const { onCreated } = show();
    mockSave.mockResolvedValueOnce(null);
    await addFiles(["/in/a.txt"]);
    fireEvent.click(screen.getByRole("button", { name: "Create…" }));
    await waitFor(() => expect(mockSave).toHaveBeenCalled());
    expect(mockCreate).not.toHaveBeenCalled();
    expect(onCreated).not.toHaveBeenCalled();
  });

  it("shows each warning as a toast", async () => {
    mockCreate.mockResolvedValueOnce({ ...summary, warnings: ["old.txt is not UTF-8, so it was read as Western (Windows-1252)."] });
    show();
    await addFiles(["/in/old.txt"]);
    fireEvent.click(screen.getByRole("button", { name: "Create…" }));
    await waitFor(() => expect(useToastStore.getState().toasts).toHaveLength(1));
    expect(useToastStore.getState().toasts[0]).toMatchObject({ kind: "info" });
  });

  it("reports a failure instead of opening a file that wasn't written", async () => {
    mockCreate.mockRejectedValueOnce(new Error("archive.zip isn't a JPEG, PNG, TIFF or WebP image or a plain-text file."));
    const { onCreated } = show();
    await addFiles(["/in/archive.zip"]);
    fireEvent.click(screen.getByRole("button", { name: "Create…" }));
    await waitFor(() => expect(mockReport).toHaveBeenCalled());
    expect(mockReport.mock.calls[0]?.[0]).toBe("Couldn't create the PDF");
    expect(onCreated).not.toHaveBeenCalled();
  });
});

describe("suggestedName", () => {
  it("offers Word documents, web pages, images and text in the picker", async () => {
    show();
    await addFiles(["/in/page.html"]);
    const filters = mockOpen.mock.calls[0]?.[0]?.filters ?? [];
    const offered = filters.flatMap((f) => f.extensions);
    for (const extension of ["docx", "html", "htm", "png", "jpg", "txt"]) expect(offered).toContain(extension);
  });

  it("sends PDF/A only when it is ticked", async () => {
    show();
    await addFiles(["/in/a.png"]);
    fireEvent.click(screen.getByRole("button", { name: "Create…" }));
    await waitFor(() => expect(mockCreate).toHaveBeenCalledTimes(1));
    expect(mockCreate.mock.calls[0]?.[3]).toBe(false);
    cleanup();
    show();
    await addFiles(["/in/a.png"]);
    fireEvent.click(screen.getByRole("checkbox", { name: "PDF/A, for archiving" }));
    fireEvent.click(screen.getByRole("button", { name: "Create…" }));
    await waitFor(() => expect(mockCreate).toHaveBeenCalledTimes(2));
    expect(mockCreate.mock.calls[1]?.[3]).toBe(true);
  });

  it("names the PDF after the first file", () => {
    expect(suggestedName(["/a/holiday photo.JPG", "/a/b.txt"])).toBe("holiday photo.pdf");
    expect(suggestedName([])).toBe("created.pdf");
    expect(suggestedName(["/a/.hidden"])).toBe("created.pdf");
  });
});
