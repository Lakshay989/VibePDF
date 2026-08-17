// SPEC: P6-SEC-006 (P6.B2b) — turning a verification report into what a person
// reads.
//
// This is where the honesty of the feature lives. A signature panel is read by
// someone deciding whether to believe a document, and the two ways to fail them
// are opposite: overstate ("Valid" on something we could not fully check) and
// they trust a forgery; understate ("Invalid" on a self-signed test certificate)
// and they learn to ignore the panel, which costs them the one time it matters.
//
// So the rules here are:
//
//   - "Signed by X" is never said without the certificate having been checked.
//   - **Trust is never claimed.** We have no trust anchors — see
//     `security/verify.rs`. The most we say is who the certificate claims to be
//     and that nobody has vouched for it.
//   - A broken document and a broken signature get different words, because
//     they are different events with different responses.

import type { ChainStatus, SignatureReport } from "@/ipc/pdf";

/** How much weight to give the row, and what colour it earns. */
export type Severity = "valid" | "warning" | "invalid";

export interface SignatureStatus {
  severity: Severity;
  /** The headline, e.g. "Signed by VibePDF Test Signer". */
  headline: string;
  /** Everything qualifying it, most important first. */
  notes: string[];
}

function chainNote(chain: ChainStatus): string {
  switch (chain) {
    case "selfSigned":
      return "The certificate vouches for itself — nobody else has verified it.";
    case "issuerNotChecked":
      return "Issued by another certificate, which VibePDF can't check against any trust list.";
    case "incomplete":
      return "The document doesn't include the certificates needed to follow the chain.";
    case "broken":
      return "A certificate doesn't match the key that supposedly issued it.";
  }
}

/**
 * SPEC: P6-SEC-006 — the four statuses, as a line a person can act on.
 *
 * The ordering of the checks is the point. A broken chain outranks everything
 * because it makes the *signer's name* untrustworthy, and a name is what a
 * reader actually goes by; reporting "Signed by Alice — certificate altered"
 * would put the reassuring half first.
 */
export function describeSignature(report: SignatureReport): SignatureStatus {
  const who = report.signer || "an unnamed certificate";
  const notes: string[] = [];

  if (report.problems.length > 0) {
    return {
      severity: "invalid",
      headline: "This signature could not be checked",
      notes: report.problems,
    };
  }

  // Order matters below: the worst thing first, because it changes what the
  // rest of the row means.
  if (report.chain === "broken") {
    return {
      severity: "invalid",
      headline: "This signature's certificate has been altered",
      notes: [
        chainNote(report.chain),
        "Whatever name it shows cannot be relied on.",
      ],
    };
  }

  if (!report.signatureValid) {
    return {
      severity: "invalid",
      headline: "This signature is not valid",
      notes: [
        "The signature doesn't match the certificate attached to it.",
        chainNote(report.chain),
      ],
    };
  }

  if (!report.digestMatches) {
    return {
      severity: "invalid",
      headline: `The document changed after ${who} signed it`,
      notes: [
        "The signature itself is intact — it is the document that no longer matches.",
        chainNote(report.chain),
      ],
    };
  }

  // From here the mathematics is sound. What remains is what we could not
  // establish, which is a warning rather than a failure.
  if (!report.coversWholeDocument) {
    notes.push(
      "Something was added to the file after this signature — often a second signature, but it is outside what this one covers.",
    );
  }
  if (report.certificateExpired) {
    notes.push("The certificate has expired. That does not undo the signature, but nobody is standing behind the certificate now.");
  }
  if (report.certificationLevel !== null) {
    notes.push(certificationNote(report.certificationLevel));
  }
  notes.push(chainNote(report.chain));

  return {
    severity: notes.some((n) => n.startsWith("Something was added")) || report.certificateExpired
      ? "warning"
      : "valid",
    headline: `Signed by ${who}`,
    notes,
  };
}

function certificationNote(level: number): string {
  switch (level) {
    case 1:
      return "Certified: no changes were meant to be made after signing.";
    case 2:
      return "Certified: only form filling and further signatures were meant to be allowed.";
    case 3:
      return "Certified: form filling, comments and further signatures were meant to be allowed.";
    default:
      return `Certified at an unrecognised level (${level}).`;
  }
}

/** The one line for a whole document, for a banner. */
export function summarise(reports: SignatureReport[]): SignatureStatus | null {
  if (reports.length === 0) return null;

  const each = reports.map(describeSignature);
  const worst: Severity = each.some((s) => s.severity === "invalid")
    ? "invalid"
    : each.some((s) => s.severity === "warning")
      ? "warning"
      : "valid";

  if (reports.length === 1) return { ...each[0]!, severity: worst };

  const bad = each.filter((s) => s.severity !== "valid").length;
  return {
    severity: worst,
    headline:
      bad === 0
        ? `${reports.length} signatures, all intact`
        : `${reports.length} signatures, ${bad} needing attention`,
    notes: [],
  };
}
