// SPEC: P6-SEC-006 (P6.B2b) — the signature status banner.

import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";

import { SignatureBanner } from "@/app/SignatureBanner";
import type { SignatureReport } from "@/ipc/pdf";

const intact = (over: Partial<SignatureReport> = {}): SignatureReport => ({
  fieldName: "Signature1",
  signer: "CN=VibePDF Test Signer",
  issuer: "CN=VibePDF Test Signer",
  signedAt: "D:20260813104500+00'00'",
  reason: "I approve this document",
  signatureValid: true,
  digestMatches: true,
  coversWholeDocument: true,
  certificateExpired: false,
  chain: "selfSigned",
  certificationLevel: null,
  problems: [],
  ...over,
});

const onDismiss = vi.fn();
const banner = (reports: SignatureReport[]) => (
  <SignatureBanner reports={reports} dismissed={false} onDismiss={onDismiss} />
);

afterEach(cleanup);

describe("SignatureBanner", () => {
  // The overwhelming majority of documents are unsigned, and a banner on every
  // one of them is a banner nobody reads.
  it("shows nothing for an unsigned document", () => {
    const { container } = render(banner([]));
    expect(container.firstChild).toBeNull();
  });

  it("shows nothing once dismissed", () => {
    const { container } = render(
      <SignatureBanner reports={[intact()]} dismissed onDismiss={onDismiss} />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("names the signer for an intact signature", () => {
    render(banner([intact()]));
    expect(screen.getByText("Signed")).toBeTruthy();
    expect(screen.getByText(/VibePDF Test Signer/)).toBeTruthy();
  });

  // The label a user glances at has to change with the finding, not just the
  // colour — colour alone is unavailable to a lot of readers.
  it("labels problems in words, not only in colour", () => {
    cleanup();
    render(banner([intact({ digestMatches: false })]));
    expect(screen.getByText("Signature problem")).toBeTruthy();

    cleanup();
    render(banner([intact({ certificateExpired: true })]));
    expect(screen.getByText("Signed, with caveats")).toBeTruthy();
  });

  it("lists each signature's detail on request", () => {
    render(banner([intact(), intact({ fieldName: "Signature2", digestMatches: false })]));
    expect(screen.queryByText(/Claimed signing time/)).toBeNull();

    fireEvent.click(screen.getByText("Details"));
    expect(screen.getAllByText(/Claimed signing time/).length).toBe(2);
    expect(screen.getByText(/document changed after/i)).toBeTruthy();
  });

  // "Claimed", not "Signed at": without a timestamp token the time is whatever
  // the signer's clock said, and a panel that states it as fact is asserting
  // something it cannot check.
  it("calls the signing time claimed", () => {
    render(banner([intact()]));
    fireEvent.click(screen.getByText("Details"));
    expect(screen.getByText(/Claimed signing time/)).toBeTruthy();
  });

  it("can be dismissed", () => {
    render(banner([intact()]));
    fireEvent.click(screen.getByLabelText("Dismiss signature status"));
    expect(onDismiss).toHaveBeenCalled();
  });
});
