// SPEC: P7-OCR-001/002/003 — the OCR dialog.
//
// What is worth pinning here is what the user's choices *become*: the pages
// list (empty means the whole document, which is easy to invert), the quality
// switches, which are the only way to reach P7-OCR-003, and the download
// switch, which is the application's single consent moment for network access.
//
// Also pinned: the dialog is inert while a run is in flight. OCR takes seconds
// per page, so a second click is an easy accident and would start a second run
// over a document the first is still editing.

import type { ComponentProps } from "react";

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

vi.mock("@/ipc/ocr", () => ({ runOcr: vi.fn() }));
vi.mock("@/ipc/languages", () => ({
  listLanguages: vi.fn(),
  downloadsAllowed: vi.fn(),
  setDownloadsAllowed: vi.fn(),
  installLanguageFile: vi.fn(),
  removeLanguage: vi.fn(),
}));
vi.mock("@/app/report-error", () => ({ reportError: vi.fn() }));

import { OcrDialog } from "@/app/OcrDialog";
import { reportError } from "@/app/report-error";
import {
  downloadsAllowed,
  type LanguagePack,
  listLanguages,
  removeLanguage,
  setDownloadsAllowed,
} from "@/ipc/languages";
import { type OcrSummary, runOcr } from "@/ipc/ocr";

const mockRun = vi.mocked(runOcr);
const mockList = vi.mocked(listLanguages);
const mockAllowed = vi.mocked(downloadsAllowed);
const mockSetAllowed = vi.mocked(setDownloadsAllowed);
const mockRemove = vi.mocked(removeLanguage);
const mockReport = vi.mocked(reportError);

const pack = (code: string, name: string, source: "bundled" | "added" = "bundled"): LanguagePack => ({
  code,
  name,
  source,
  sizeBytes: 1024,
});

const summary = (over: Partial<OcrSummary> = {}): OcrSummary => ({
  pages: 1,
  words: 27,
  skipped: 0,
  milliseconds: 4500,
  ...over,
});

const show = (props: Partial<ComponentProps<typeof OcrDialog>> = {}) =>
  render(
    <OcrDialog
      open
      documentId={"doc-1" as never}
      currentPage={2}
      pageCount={5}
      onClose={props.onClose ?? vi.fn()}
      {...props}
    />,
  );

/** The dialog loads languages on open; wait for that before interacting. */
const ready = async () => {
  await waitFor(() => expect(screen.getByRole("combobox")).toBeTruthy());
};

beforeEach(() => {
  vi.clearAllMocks();
  mockList.mockResolvedValue([pack("eng", "English"), pack("rus", "Russian")]);
  mockAllowed.mockResolvedValue(false);
  mockRun.mockResolvedValue({ summary: summary(), history: { canUndo: true, canRedo: false } });
});
afterEach(cleanup);

describe("OcrDialog", () => {
  it("lists the installed languages", async () => {
    show();
    await ready();
    expect(screen.getByRole("option", { name: "English" })).toBeTruthy();
    expect(screen.getByRole("option", { name: "Russian" })).toBeTruthy();
  });

  it("reads the whole document by default", async () => {
    // An empty page list means "all of them" on the Rust side. Inverting this
    // would silently OCR one page of a fifty-page scan.
    show();
    await ready();
    fireEvent.click(screen.getByRole("button", { name: "Read text" }));
    await waitFor(() => expect(mockRun).toHaveBeenCalled());
    expect(mockRun.mock.calls[0]?.[1]).toEqual([]);
  });

  it("reads only the page on screen when asked", async () => {
    show();
    await ready();
    fireEvent.click(screen.getByLabelText("This page only"));
    fireEvent.click(screen.getByRole("button", { name: "Read text" }));
    await waitFor(() => expect(mockRun).toHaveBeenCalled());
    expect(mockRun.mock.calls[0]?.[1]).toEqual([2]);
  });

  it("passes the chosen language and quality switches through", async () => {
    // SPEC: P7-OCR-003 — these switches are the only way to reach the
    // preprocessing options the spec calls configurable.
    show();
    await ready();
    fireEvent.change(screen.getByRole("combobox"), { target: { value: "rus" } });
    fireEvent.click(screen.getByLabelText(/Straighten crooked pages/));
    fireEvent.click(screen.getByLabelText(/Enlarge low-resolution pages/));
    fireEvent.click(screen.getByRole("button", { name: "Read text" }));

    await waitFor(() => expect(mockRun).toHaveBeenCalled());
    expect(mockRun.mock.calls[0]?.[2]).toEqual({
      language: "rus",
      deskew: false,
      denoise: true,
      minDpi: 0,
    });
  });

  it("says what it read when it finishes", async () => {
    show();
    await ready();
    fireEvent.click(screen.getByRole("button", { name: "Read text" }));
    await waitFor(() => expect(screen.getByText(/Read 27 words on 1 page/)).toBeTruthy());
  });

  it("says so when a page turns out to have nothing readable", async () => {
    mockRun.mockResolvedValue({
      summary: summary({ words: 0 }),
      history: { canUndo: false, canRedo: false },
    });
    show();
    await ready();
    fireEvent.click(screen.getByRole("button", { name: "Read text" }));
    await waitFor(() => expect(screen.getByText(/Nothing was readable/)).toBeTruthy());
  });

  it("reports the reason when a run fails", async () => {
    mockRun.mockRejectedValue(new Error("This document is password protected"));
    show();
    await ready();
    fireEvent.click(screen.getByRole("button", { name: "Read text" }));
    await waitFor(() => expect(mockReport).toHaveBeenCalled());
    expect(mockReport.mock.calls[0]?.[0]).toBe("Couldn't read this document");
  });

  it("stays inert while a run is in flight", async () => {
    let finish: (value: { summary: OcrSummary; history: { canUndo: boolean; canRedo: boolean } }) => void = () => {};
    mockRun.mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    show();
    await ready();
    const button = screen.getByRole("button", { name: "Read text" });
    fireEvent.click(button);

    await waitFor(() => expect(screen.getByRole("button", { name: "Reading…" })).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: "Reading…" }));
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(mockRun).toHaveBeenCalledTimes(1);

    finish({ summary: summary(), history: { canUndo: true, canRedo: false } });
    await waitFor(() => expect(screen.getByText(/Read 27 words/)).toBeTruthy());
  });

  it("keeps downloads off until the switch is turned on", async () => {
    // The application's only network call. It must start off, and turning it
    // on must be a deliberate act rather than a side effect of opening this.
    show();
    await ready();
    fireEvent.click(screen.getByText("Manage languages"));
    const toggle = screen.getByLabelText(/Allow downloading language packs/);
    expect((toggle as HTMLInputElement).checked).toBe(false);
    expect(mockSetAllowed).not.toHaveBeenCalled();

    mockSetAllowed.mockResolvedValue(true);
    fireEvent.click(toggle);
    await waitFor(() => expect(mockSetAllowed).toHaveBeenCalledWith(true));
  });

  it("shows the download switch as the backend reports it", async () => {
    // The switch is a view of a persisted setting, not a local default: if the
    // two ever disagree, the dialog says "off" while the app can download.
    mockAllowed.mockResolvedValue(true);
    show();
    await ready();
    fireEvent.click(screen.getByText("Manage languages"));
    await waitFor(() => {
      const toggle = screen.getByLabelText(/Allow downloading language packs/) as HTMLInputElement;
      expect(toggle.checked).toBe(true);
    });
  });

  it("only offers to remove languages the user added", async () => {
    mockList.mockResolvedValue([pack("eng", "English"), pack("ell", "ell", "added")]);
    show();
    await ready();
    fireEvent.click(screen.getByText("Manage languages"));
    const remove = screen.getAllByRole("button", { name: "Remove" });
    expect(remove).toHaveLength(1);

    fireEvent.click(remove[0] as HTMLElement);
    await waitFor(() => expect(mockRemove).toHaveBeenCalledWith("ell"));
  });
});
