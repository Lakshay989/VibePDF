// SPEC: P6-SEC-007 (P6.C1) — the password-protect dialog.
//
// The backend refuses a request with no passwords, so these tests are not the
// last line of defence; they are about the dialog not *offering* to do the
// wrong thing, and about the two passwords keeping their distinct meanings on
// the way out. A user password sent as an owner password would produce a file
// that opens for anyone — no error anywhere, just no protection.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn() }));
vi.mock("@/ipc/pdf", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/ipc/pdf")>()),
  protectPdf: vi.fn(),
}));
vi.mock("@/app/report-error", () => ({ reportError: vi.fn() }));

import { save as saveFileDialog } from "@tauri-apps/plugin-dialog";

import { ProtectDialog } from "@/app/ProtectDialog";
import { reportError } from "@/app/report-error";
import { ALL_PERMISSIONS, protectPdf } from "@/ipc/pdf";

const mockSave = vi.mocked(saveFileDialog);
const mockProtect = vi.mocked(protectPdf);
const mockReport = vi.mocked(reportError);

/** The labels the dialog renders, paired with the field each one sets. */
const PERMISSIONS: ReadonlyArray<[keyof typeof ALL_PERMISSIONS, string]> = [
  ["print", "Printing"],
  ["copy", "Copying text and graphics"],
  ["modify", "Changing the document"],
  ["fillForms", "Filling in form fields"],
  ["annotate", "Adding comments and annotations"],
  ["extract", "Extracting for accessibility"],
  ["assemble", "Assembling pages"],
];

const onClose = vi.fn();
const dialog = () => (
  <ProtectDialog open documentId="doc-1" documentName="/tmp/report.pdf" onClose={onClose} />
);

const type = (label: string, value: string) =>
  fireEvent.change(screen.getByLabelText(label), { target: { value } });

beforeEach(() => {
  mockSave.mockResolvedValue("/tmp/report-protected.pdf" as never);
  mockProtect.mockResolvedValue(undefined);
  vi.clearAllMocks();
});
afterEach(cleanup);

describe("ProtectDialog", () => {
  it("renders nothing when closed", () => {
    const { container } = render(
      <ProtectDialog open={false} documentId="doc-1" onClose={onClose} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("will not protect with no password at all", () => {
    render(dialog());
    expect(screen.getByText("Protect…").hasAttribute("disabled")).toBe(true);
    // …and says why, rather than leaving a dead button.
    expect(screen.getByText(/Set a password to open the document/i)).toBeTruthy();
  });

  it("blocks on a mismatched confirmation", () => {
    render(dialog());
    type("Password to open", "hunter2");
    type("Confirm password to open", "hunter3");

    expect(screen.getByText("Protect…").hasAttribute("disabled")).toBe(true);
    expect(screen.getByText(/do not match/i)).toBeTruthy();
  });

  it("sends the open password as the user password, not the owner one", async () => {
    // The distinction that matters: swapping these produces a document that
    // opens for anyone, with nothing to indicate the protection failed.
    render(dialog());
    type("Password to open", "hunter2");
    type("Confirm password to open", "hunter2");
    fireEvent.click(screen.getByText("Protect…"));

    await waitFor(() => expect(mockProtect).toHaveBeenCalled());
    expect(mockProtect).toHaveBeenCalledWith(
      "doc-1",
      "/tmp/report-protected.pdf",
      "hunter2",
      null,
      ALL_PERMISSIONS,
    );
  });

  it("refuses an owner password on its own", () => {
    // A deliberate narrowing of P6-SEC-007: P6.C2 cannot unlock a document that
    // has only a permissions password, so we do not write one. Producing files
    // we cannot undo is worse than a missing option, and the user would meet it
    // later, on a document they can no longer change.
    render(dialog());
    type("Password to change permissions", "let-me-in");

    expect(screen.getByText("Protect…").hasAttribute("disabled")).toBe(true);
    expect(screen.getByText(/could not be removed afterwards/i)).toBeTruthy();
  });

  it("sends both passwords when both are given", async () => {
    render(dialog());
    type("Password to open", "open-me");
    type("Confirm password to open", "open-me");
    type("Password to change permissions", "let-me-in");
    fireEvent.click(screen.getByText("Protect…"));

    await waitFor(() => expect(mockProtect).toHaveBeenCalled());
    expect(mockProtect).toHaveBeenCalledWith(
      "doc-1",
      "/tmp/report-protected.pdf",
      "open-me",
      "let-me-in",
      ALL_PERMISSIONS,
    );
  });

  it("does nothing when the save dialog is dismissed", async () => {
    mockSave.mockResolvedValue(null as never);
    render(dialog());
    type("Password to open", "pw");
    type("Confirm password to open", "pw");
    fireEvent.click(screen.getByText("Protect…"));

    await waitFor(() => expect(mockSave).toHaveBeenCalled());
    expect(mockProtect).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("keeps the dialog open and reports when protecting fails", async () => {
    mockProtect.mockRejectedValue(new Error("could not write"));
    render(dialog());
    type("Password to open", "pw");
    type("Confirm password to open", "pw");
    fireEvent.click(screen.getByText("Protect…"));

    await waitFor(() => expect(mockReport).toHaveBeenCalled());
    expect(onClose).not.toHaveBeenCalled();
  });

  it("masks every password field", () => {
    render(dialog());
    type("Password to open", "pw");
    for (const label of [
      "Password to open",
      "Confirm password to open",
      "Password to change permissions",
    ]) {
      expect((screen.getByLabelText(label) as HTMLInputElement).type).toBe("password");
    }
  });

  // SPEC: P6-SEC-009 (P6.C3) — permissions.
  //
  // The checkbox sense is the thing to get wrong here: ticked means *allowed*.
  // An inverted box would send the opposite of what the user saw, and nothing
  // downstream could tell — a document restricting everything is as valid as
  // one restricting nothing.
  it("starts with every permission granted", () => {
    render(dialog());
    for (const [, label] of PERMISSIONS) {
      expect((screen.getByLabelText(label) as HTMLInputElement).checked).toBe(true);
    }
  });

  it("sends a cleared box as a withheld permission", async () => {
    render(dialog());
    type("Password to open", "pw");
    type("Confirm password to open", "pw");
    fireEvent.click(screen.getByLabelText("Printing"));
    fireEvent.click(screen.getByLabelText("Copying text and graphics"));
    fireEvent.click(screen.getByText("Protect…"));

    await waitFor(() => expect(mockProtect).toHaveBeenCalled());
    expect(mockProtect.mock.calls[0]?.[4]).toEqual({
      ...ALL_PERMISSIONS,
      print: false,
      copy: false,
    });
  });

  it("covers all seven permissions the spec names", () => {
    render(dialog());
    expect(PERMISSIONS).toHaveLength(7);
    for (const [, label] of PERMISSIONS) {
      expect(screen.getByLabelText(label)).toBeTruthy();
    }
  });

  // Restrictions with no distinct owner password can be lifted by anyone who
  // can open the file. Saying so is the difference between a feature and a
  // false sense of security.
  it("warns that restrictions are liftable without a permissions password", () => {
    render(dialog());
    expect(screen.queryByText(/can also lift these restrictions/i)).toBeNull();

    fireEvent.click(screen.getByLabelText("Printing"));
    expect(screen.getByText(/can also lift these restrictions/i)).toBeTruthy();

    type("Password to change permissions", "owner-only");
    expect(screen.queryByText(/can also lift these restrictions/i)).toBeNull();
  });

  it("does not keep the permissions after cancelling", () => {
    render(dialog());
    fireEvent.click(screen.getByLabelText("Printing"));
    fireEvent.click(screen.getByText("Cancel"));

    expect((screen.getByLabelText("Printing") as HTMLInputElement).checked).toBe(true);
  });

  it("does not keep the passwords after cancelling", () => {
    render(dialog());
    type("Password to open", "hunter2");
    fireEvent.click(screen.getByText("Cancel"));

    expect(onClose).toHaveBeenCalled();
    // Same mounted component, reopened: the field must be empty rather than
    // holding someone's password from the last time.
    expect((screen.getByLabelText("Password to open") as HTMLInputElement).value).toBe("");
  });
});
