// SPEC: P6-SEC-005 (P6.B1a) — the certificate-signing dialog.
//
// The two things worth pinning: the certificate password must never survive the
// dialog, and the four arguments must arrive in their intended roles. A reason
// sent as a location is harmless; a password sent as a path is not, and neither
// mistake produces an error anywhere.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }));
vi.mock("@/ipc/pdf", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/ipc/pdf")>()),
  signPdf: vi.fn(),
  unsignedSignatureFields: vi.fn(),
}));
vi.mock("@/app/report-error", () => ({ reportError: vi.fn() }));

import { open as openFileDialog, save as saveFileDialog } from "@tauri-apps/plugin-dialog";

import { reportError } from "@/app/report-error";
import { SignDialog } from "@/app/SignDialog";
import { signPdf, unsignedSignatureFields } from "@/ipc/pdf";

const mockOpen = vi.mocked(openFileDialog);
const mockSave = vi.mocked(saveFileDialog);
const mockSign = vi.mocked(signPdf);
const mockFields = vi.mocked(unsignedSignatureFields);
const mockReport = vi.mocked(reportError);

const onClose = vi.fn();
const dialog = () => (
  <SignDialog open documentId="doc-1" documentName="/tmp/report.pdf" onClose={onClose} />
);

/**
 * Render and let the field-listing effect settle.
 *
 * The dialog asks the backend which signature fields the document has as soon
 * as it opens, so a bare `render` leaves a state update landing outside
 * `act()` — a warning on every test rather than a real failure, which is
 * exactly the kind of noise that hides the next real one.
 */
const show = async () => {
  render(dialog());
  // Flush the pending field-listing promise. An empty `act` rather than one
  // wrapping `render`, so a test that then awaits its own act does not nest.
  await act(async () => {});
};

const type = (label: string, value: string) =>
  fireEvent.change(screen.getByLabelText(label), { target: { value } });

/** Pick a certificate and give its password — the minimum to enable signing. */
const readyToSign = async () => {
  fireEvent.click(screen.getByText("Choose…"));
  await waitFor(() => expect(screen.getByText("signer.pfx")).toBeTruthy());
  type("Certificate password", "test123");
};

beforeEach(() => {
  vi.clearAllMocks();
  mockOpen.mockResolvedValue("/certs/signer.pfx" as never);
  mockSave.mockResolvedValue("/tmp/report-signed.pdf" as never);
  mockSign.mockResolvedValue(undefined);
  // Most documents have no signature field; the ones that do get their own tests.
  mockFields.mockResolvedValue([]);
});
afterEach(cleanup);

describe("SignDialog", () => {
  it("renders nothing when closed", async () => {
    const { container } = render(
      <SignDialog open={false} documentId="doc-1" onClose={onClose} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("will not sign without a certificate", async () => {
    await show();
    type("Certificate password", "test123");
    expect(screen.getByText("Sign…").hasAttribute("disabled")).toBe(true);
    expect(screen.getByText(/Choose the certificate/i)).toBeTruthy();
  });

  it("will not sign without a password", async () => {
    await show();
    fireEvent.click(screen.getByText("Choose…"));
    await waitFor(() => expect(screen.getByText("signer.pfx")).toBeTruthy());
    expect(screen.getByText("Sign…").hasAttribute("disabled")).toBe(true);
  });

  it("sends each field in its own role", async () => {
    await show();
    await readyToSign();
    type("Reason", "I approve this document");
    type("Location", "Manchester");
    type("Name shown on the signature", "L. Kucheriya");
    fireEvent.click(screen.getByText("Sign…"));

    await waitFor(() => expect(mockSign).toHaveBeenCalled());
    const [id, out, cert, password, details] = mockSign.mock.calls[0] ?? [];
    expect(id).toBe("doc-1");
    expect(out).toBe("/tmp/report-signed.pdf");
    expect(cert).toBe("/certs/signer.pfx");
    expect(password).toBe("test123");
    expect(details).toMatchObject({
      reason: "I approve this document",
      location: "Manchester",
      name: "L. Kucheriya",
    });
  });

  // The backend wants a PDF date string; anything else lands in /M unvalidated.
  it("sends the signing time as a PDF date", async () => {
    await show();
    await readyToSign();
    fireEvent.click(screen.getByText("Sign…"));

    await waitFor(() => expect(mockSign).toHaveBeenCalled());
    const details = mockSign.mock.calls[0]?.[4];
    expect(details?.signedAt).toMatch(/^D:\d{14}(Z|[+-]\d{2})'\d{2}'$/);
  });

  // SPEC: P6-SEC-005 (P6.B1b) — certification.
  //
  // Certifying by accident is the dangerous direction: it makes a claim about
  // the whole document that the signer never made. So the default has to be a
  // plain approval signature, and that is worth a test rather than a glance.
  it("signs without certifying unless asked", async () => {
    await show();
    await readyToSign();
    fireEvent.click(screen.getByText("Sign…"));

    await waitFor(() => expect(mockSign).toHaveBeenCalled());
    expect(mockSign.mock.calls[0]?.[4]?.certify).toBeNull();
  });

  it("sends the chosen certification level", async () => {
    await show();
    await readyToSign();
    fireEvent.change(screen.getByLabelText("After signing"), {
      target: { value: "formFilling" },
    });
    fireEvent.click(screen.getByText("Sign…"));

    await waitFor(() => expect(mockSign).toHaveBeenCalled());
    expect(mockSign.mock.calls[0]?.[4]?.certify).toBe("formFilling");
  });

  // "Lock" sounds like prevention. It is not, and the dialog should not imply
  // it is — a user who believes the document cannot be changed is worse off
  // than one who knows changes will be detected.
  it("describes certification as detecting changes, not preventing them", async () => {
    await show();
    expect(screen.getByText(/says nothing about later changes/i)).toBeTruthy();

    fireEvent.change(screen.getByLabelText("After signing"), {
      target: { value: "noChanges" },
    });
    expect(screen.getByText(/detects changes rather than preventing them/i)).toBeTruthy();
  });

  it("sends empty optional fields as null rather than empty strings", async () => {
    await show();
    await readyToSign();
    fireEvent.click(screen.getByText("Sign…"));

    await waitFor(() => expect(mockSign).toHaveBeenCalled());
    expect(mockSign.mock.calls[0]?.[4]).toMatchObject({
      reason: null,
      location: null,
      name: null,
      certify: null,
    });
  });

  it("does nothing when the save dialog is dismissed", async () => {
    mockSave.mockResolvedValue(null as never);
    await show();
    await readyToSign();
    fireEvent.click(screen.getByText("Sign…"));

    await waitFor(() => expect(mockSave).toHaveBeenCalled());
    expect(mockSign).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("keeps the dialog open and reports when signing fails", async () => {
    mockSign.mockRejectedValue(new Error("that password doesn't open this file"));
    await show();
    await readyToSign();
    fireEvent.click(screen.getByText("Sign…"));

    await waitFor(() => expect(mockReport).toHaveBeenCalled());
    expect(onClose).not.toHaveBeenCalled();
  });

  it("masks the certificate password", async () => {
    await show();
    expect((screen.getByLabelText("Certificate password") as HTMLInputElement).type).toBe(
      "password",
    );
  });

  // A certificate password left in state past its use is free risk, and the
  // next document is not necessarily signed by the same person.
  it("does not keep the password or the certificate after cancelling", async () => {
    await show();
    await readyToSign();
    fireEvent.click(screen.getByText("Cancel"));

    expect(onClose).toHaveBeenCalled();
    expect((screen.getByLabelText("Certificate password") as HTMLInputElement).value).toBe("");
    expect(screen.getByText("No certificate chosen")).toBeTruthy();
  });

  it("does not keep them after a successful signing either", async () => {
    await show();
    await readyToSign();
    fireEvent.click(screen.getByText("Sign…"));

    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect((screen.getByLabelText("Certificate password") as HTMLInputElement).value).toBe("");
  });

  // SPEC: P6-SEC-004 (P6.A5b) — signing into a field the document already has.
  //
  // A document routed for sign-off arrives with an empty box in it. Signing an
  // invisible field beside that box satisfies "the document is signed" while
  // leaving the thing the recipient looks at empty, so when a field exists it
  // is the default rather than an option to go and find.
  it("offers no field picker when the document has none", async () => {
    await show();
    await waitFor(() => expect(mockFields).toHaveBeenCalled());
    expect(screen.queryByLabelText("Signature field")).toBeNull();
  });

  it("defaults to the document's first empty signature field", async () => {
    mockFields.mockResolvedValue(["Approval", "Countersign"]);
    await show();
    await waitFor(() => expect(screen.getByLabelText("Signature field")).toBeTruthy());
    await readyToSign();
    await act(async () => {
      fireEvent.click(screen.getByText("Sign…"));
    });

    expect(mockSign.mock.calls[0]?.[4]?.target).toEqual({
      kind: "existingField",
      name: "Approval",
    });
  });

  it("can still add a new invisible signature instead", async () => {
    mockFields.mockResolvedValue(["Approval"]);
    await show();
    await waitFor(() => expect(screen.getByLabelText("Signature field")).toBeTruthy());
    fireEvent.change(screen.getByLabelText("Signature field"), { target: { value: "" } });
    await readyToSign();
    await act(async () => {
      fireEvent.click(screen.getByText("Sign…"));
    });

    expect(mockSign.mock.calls[0]?.[4]?.target).toEqual({ kind: "newField" });
  });

  // Failing to list fields must not stop someone signing — the fallback is the
  // behaviour every document had before this existed.
  it("still signs when the fields cannot be listed", async () => {
    mockFields.mockRejectedValue(new Error("nope"));
    await show();
    await readyToSign();
    await act(async () => {
      fireEvent.click(screen.getByText("Sign…"));
    });

    expect(mockSign).toHaveBeenCalled();
    expect(mockSign.mock.calls[0]?.[4]?.target).toEqual({ kind: "newField" });
  });

  // Signing writes a copy on purpose: saving over the original would
  // re-serialise it and break the signature. The dialog has to say so.
  it("says the open document is not changed", async () => {
    await show();
    expect(screen.getByText(/document you have open is not changed/i)).toBeTruthy();
  });
});
