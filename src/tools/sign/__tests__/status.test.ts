// SPEC: P6-SEC-006 (P6.B2b) — what the signature panel says.
//
// These are tests about *wording*, which is unusual and deliberate. A signature
// panel is read by someone deciding whether to believe a document, and the two
// failure modes are opposite: overstate and they trust a forgery; understate and
// they learn to ignore the panel, which costs them the one time it matters.
//
// The specific things pinned here:
//   - "Signed by X" is never shown when X came from an unchecked certificate.
//   - The word "trusted" never appears at all.
//   - A changed document and a bad signature get different sentences.

import { describe, expect, it } from "vitest";

import type { SignatureReport } from "@/ipc/pdf";
import { describeSignature, summarise } from "@/tools/sign/status";

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

describe("describeSignature", () => {
  it("names the signer when everything checks out", () => {
    const s = describeSignature(intact());
    expect(s.severity).toBe("valid");
    expect(s.headline).toContain("VibePDF Test Signer");
  });

  // Even at its most positive, the panel must not imply anyone vouched for the
  // certificate. We have no trust anchors; saying otherwise is the one thing
  // this feature must never do.
  it("never claims a signature is trusted", () => {
    for (const report of [
      intact(),
      intact({ chain: "issuerNotChecked" }),
      intact({ certificateExpired: true }),
      intact({ certificationLevel: 1 }),
    ]) {
      const s = describeSignature(report);
      const text = [s.headline, ...s.notes].join(" ").toLowerCase();
      expect(text).not.toContain("trusted");
      expect(text).not.toContain("verified issuer");
    }
  });

  it("always says something about who vouched for the certificate", () => {
    const s = describeSignature(intact());
    expect(s.notes.join(" ")).toMatch(/vouches for itself|nobody else/i);
  });

  // A changed document and a forged signature are different events with
  // different responses, so they must not share a sentence.
  it("distinguishes a changed document from a bad signature", () => {
    const changed = describeSignature(intact({ digestMatches: false }));
    const forged = describeSignature(intact({ signatureValid: false }));

    expect(changed.severity).toBe("invalid");
    expect(changed.headline).toMatch(/document changed/i);
    expect(changed.notes.join(" ")).toMatch(/signature itself is intact/i);

    expect(forged.severity).toBe("invalid");
    expect(forged.headline).toMatch(/not valid/i);
    expect(forged.headline).not.toMatch(/document changed/i);
  });

  // A tampered certificate makes the *name* untrustworthy, so it has to outrank
  // everything — "Signed by Alice, certificate altered" leads with the
  // reassuring half.
  it("leads with the certificate when the certificate is the problem", () => {
    const s = describeSignature(intact({ chain: "broken" }));
    expect(s.severity).toBe("invalid");
    expect(s.headline).toMatch(/certificate has been altered/i);
    expect(s.headline).not.toContain("VibePDF Test Signer");
    expect(s.notes.join(" ")).toMatch(/cannot be relied on/i);
  });

  it("treats an appended file as a warning, not a failure", () => {
    const s = describeSignature(intact({ coversWholeDocument: false }));
    expect(s.severity).toBe("warning");
    expect(s.notes.join(" ")).toMatch(/added to the file after/i);
  });

  // Expiry does not retroactively break the mathematics, and saying so stops a
  // reader concluding the document was forged.
  it("says an expired certificate does not undo the signature", () => {
    const s = describeSignature(intact({ certificateExpired: true }));
    expect(s.severity).toBe("warning");
    expect(s.notes.join(" ")).toMatch(/does not undo the signature/i);
  });

  it("reports the certification level in words", () => {
    expect(describeSignature(intact({ certificationLevel: 1 })).notes.join(" ")).toMatch(
      /no changes/i,
    );
    expect(describeSignature(intact({ certificationLevel: 3 })).notes.join(" ")).toMatch(
      /comments/i,
    );
  });

  it("surfaces problems verbatim rather than guessing", () => {
    const s = describeSignature(
      intact({ problems: ["The signature has no readable content."] }),
    );
    expect(s.severity).toBe("invalid");
    expect(s.notes).toEqual(["The signature has no readable content."]);
  });
});

describe("summarise", () => {
  it("says nothing about an unsigned document", () => {
    expect(summarise([])).toBeNull();
  });

  it("passes a single signature through", () => {
    expect(summarise([intact()])?.headline).toContain("VibePDF Test Signer");
  });

  it("counts the ones needing attention", () => {
    const s = summarise([intact(), intact({ digestMatches: false }), intact()]);
    expect(s?.severity).toBe("invalid");
    expect(s?.headline).toBe("3 signatures, 1 needing attention");
  });

  // One bad signature among many must not be averaged away by the good ones.
  it("takes the worst severity, not the most common", () => {
    expect(summarise([intact(), intact(), intact({ chain: "broken" })])?.severity).toBe(
      "invalid",
    );
    expect(summarise([intact(), intact({ certificateExpired: true })])?.severity).toBe(
      "warning",
    );
  });
});
