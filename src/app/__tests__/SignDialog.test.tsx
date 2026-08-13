// SPEC: P6-SEC-005 (P6.B1a) — the certificate-signing dialog.
//
// The two things worth pinning: the certificate password must never survive the
// dialog, and the four arguments must arrive in their intended roles. A reason
// sent as a location is harmless; a password sent as a path is not, and neither
// mistake produces an error anywhere.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }));
vi.mock("@/ipc/pdf", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@/ipc/pdf")>()),
  signPdf: vi.fn(),
}));
vi.mock("@/app/report-error", () => ({ reportError: vi.fn() }));

import { open as openFileDialog, save as saveFileDialog } from "@tauri-apps/plugin-dialog";

import { reportError } from "@/app/report-error";
import { SignDialog } from "@/app/SignDialog";
import { signPdf } from "@/ipc/pdf";

const mockOpen = vi.mocked(openFileDialog);
const mockSave = vi.mocked(saveFileDialog);
const mockSign = vi.mocked(signPdf);
const mockReport = vi.mocked(reportError);

const onClose = vi.fn();
const dialog = () => (
  <SignDialog open documentId="doc-1" documentName="/tmp/report.pdf" onClose={onClose} />
);

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
});
afterEach(cleanup);

describe("SignDialog", () => {
  it("renders nothing when closed", () => {
    const { container } = render(
      <SignDialog open={false} documentId="doc-1" onClose={onClose} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("will not sign without a certificate", () => {
    render(dialog());
    type("Certificate password", "test123");
    expect(screen.getByText("Sign…").hasAttribute("disabled")).toBe(true);
    expect(screen.getByText(/Choose the certificate/i)).toBeTruthy();
  });

  it("will not sign without a password", async () => {
    render(dialog());
    fireEvent.click(screen.getByText("Choose…"));
    await waitFor(() => expect(screen.getByText("signer.pfx")).toBeTruthy());
    expect(screen.getByText("Sign…").hasAttribute("disabled")).toBe(true);
  });

  it("sends each field in its own role", async () => {
    render(dialog());
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
    render(dialog());
    await readyToSign();
    fireEvent.click(screen.getByText("Sign…"));

    await waitFor(() => expect(mockSign).toHaveBeenCalled());
    const details = mockSign.mock.calls[0]?.[4];
    expect(details?.signedAt).toMatch(/^D:\d{14}(Z|[+-]\d{2})'\d{2}'$/);
  });

  it("sends empty optional fields as null rather than empty strings", async () => {
    render(dialog());
    await readyToSign();
    fireEvent.click(screen.getByText("Sign…"));

    await waitFor(() => expect(mockSign).toHaveBeenCalled());
    expect(mockSign.mock.calls[0]?.[4]).toMatchObject({
      reason: null,
      location: null,
      name: null,
    });
  });

  it("does nothing when the save dialog is dismissed", async () => {
    mockSave.mockResolvedValue(null as never);
    render(dialog());
    await readyToSign();
    fireEvent.click(screen.getByText("Sign…"));

    await waitFor(() => expect(mockSave).toHaveBeenCalled());
    expect(mockSign).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("keeps the dialog open and reports when signing fails", async () => {
    mockSign.mockRejectedValue(new Error("that password doesn't open this file"));
    render(dialog());
    await readyToSign();
    fireEvent.click(screen.getByText("Sign…"));

    await waitFor(() => expect(mockReport).toHaveBeenCalled());
    expect(onClose).not.toHaveBeenCalled();
  });

  it("masks the certificate password", () => {
    render(dialog());
    expect((screen.getByLabelText("Certificate password") as HTMLInputElement).type).toBe(
      "password",
    );
  });

  // A certificate password left in state past its use is free risk, and the
  // next document is not necessarily signed by the same person.
  it("does not keep the password or the certificate after cancelling", async () => {
    render(dialog());
    await readyToSign();
    fireEvent.click(screen.getByText("Cancel"));

    expect(onClose).toHaveBeenCalled();
    expect((screen.getByLabelText("Certificate password") as HTMLInputElement).value).toBe("");
    expect(screen.getByText("No certificate chosen")).toBeTruthy();
  });

  it("does not keep them after a successful signing either", async () => {
    render(dialog());
    await readyToSign();
    fireEvent.click(screen.getByText("Sign…"));

    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect((screen.getByLabelText("Certificate password") as HTMLInputElement).value).toBe("");
  });

  // Signing writes a copy on purpose: saving over the original would
  // re-serialise it and break the signature. The dialog has to say so.
  it("says the open document is not changed", () => {
    render(dialog());
    expect(screen.getByText(/document you have open is not changed/i)).toBeTruthy();
  });
});
