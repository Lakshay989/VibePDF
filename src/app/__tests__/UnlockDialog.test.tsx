// SPEC: P6-SEC-008 (P6.C2) — the remove-protection dialog.
//
// Small surface, one property worth guarding: a failure must leave the dialog
// open with the password intact. Wrong passwords are the common case here, and
// clearing the field on every miss would make a typo cost the whole entry.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn() }));
vi.mock("@/ipc/pdf", () => ({ removePdfProtection: vi.fn() }));
vi.mock("@/app/report-error", () => ({ reportError: vi.fn() }));

import { save as saveFileDialog } from "@tauri-apps/plugin-dialog";

import { reportError } from "@/app/report-error";
import { UnlockDialog } from "@/app/UnlockDialog";
import { removePdfProtection } from "@/ipc/pdf";

const mockSave = vi.mocked(saveFileDialog);
const mockRemove = vi.mocked(removePdfProtection);
const mockReport = vi.mocked(reportError);

const onClose = vi.fn();
const dialog = () => (
  <UnlockDialog open documentId="doc-1" documentName="/tmp/secret.pdf" onClose={onClose} />
);
const type = (value: string) =>
  fireEvent.change(screen.getByLabelText("The document's password"), { target: { value } });

beforeEach(() => {
  mockSave.mockResolvedValue("/tmp/secret-unlocked.pdf" as never);
  mockRemove.mockResolvedValue(undefined);
  vi.clearAllMocks();
});
afterEach(cleanup);

describe("UnlockDialog", () => {
  it("renders nothing when closed", () => {
    const { container } = render(
      <UnlockDialog open={false} documentId="doc-1" onClose={onClose} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("will not unlock without a password", () => {
    render(dialog());
    expect(screen.getByText("Unlock…").hasAttribute("disabled")).toBe(true);
  });

  it("passes the password and the chosen path through", async () => {
    render(dialog());
    type("owner-pw");
    fireEvent.click(screen.getByText("Unlock…"));

    await waitFor(() => expect(mockRemove).toHaveBeenCalled());
    expect(mockRemove).toHaveBeenCalledWith("doc-1", "/tmp/secret-unlocked.pdf", "owner-pw");
    expect(onClose).toHaveBeenCalled();
  });

  it("keeps the password when the attempt fails", async () => {
    // A wrong password is the expected failure here. Clearing the field would
    // make a single typo cost the whole entry.
    mockRemove.mockRejectedValue(new Error("That password did not unlock the document."));
    render(dialog());
    type("nearly-right");
    fireEvent.click(screen.getByText("Unlock…"));

    await waitFor(() => expect(mockReport).toHaveBeenCalled());
    expect(
      (screen.getByLabelText("The document's password") as HTMLInputElement).value,
    ).toBe("nearly-right");
    expect(onClose).not.toHaveBeenCalled();
  });

  it("does nothing when the save dialog is dismissed", async () => {
    mockSave.mockResolvedValue(null as never);
    render(dialog());
    type("pw");
    fireEvent.click(screen.getByText("Unlock…"));

    await waitFor(() => expect(mockSave).toHaveBeenCalled());
    expect(mockRemove).not.toHaveBeenCalled();
  });

  it("masks the password field", () => {
    render(dialog());
    expect(
      (screen.getByLabelText("The document's password") as HTMLInputElement).type,
    ).toBe("password");
  });

  it("says which password AES-256 needs", () => {
    // The surprising part, and the one that would otherwise send someone round
    // in circles typing the password that opens the file.
    render(dialog());
    expect(screen.getByText(/permissions password/i)).toBeTruthy();
  });
});
