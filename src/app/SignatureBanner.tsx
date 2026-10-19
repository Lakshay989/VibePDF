// SPEC: P6-SEC-006 (P6.B2b) — display per-signature status.
//
// A banner with the summary, expanding to one row per signature. Deliberately
// not a modal: the status is context for reading the document, not a decision
// to make before reading it.
//
// The colours carry meaning and so does their absence — a valid signature is
// green, an unverifiable one is amber, and a broken one is red. What none of
// them ever says is "trusted"; see `tools/sign/status.ts`.

import { useState } from "react";

import type { SignatureReport } from "@/ipc/pdf";
import { describeSignature, type Severity, summarise } from "@/tools/sign/status";

interface Props {
  reports: SignatureReport[];
  dismissed: boolean;
  onDismiss: () => void;
}

const TONE: Record<Severity, string> = {
  valid: "border-green-300 bg-green-50 text-green-900 dark:border-green-900 dark:bg-green-950 dark:text-green-200",
  warning:
    "border-amber-300 bg-amber-50 text-amber-900 dark:border-amber-900 dark:bg-amber-950 dark:text-amber-200",
  invalid: "border-red-300 bg-red-50 text-red-900 dark:border-red-900 dark:bg-red-950 dark:text-red-200",
};

const LABEL: Record<Severity, string> = {
  valid: "Signed",
  warning: "Signed, with caveats",
  invalid: "Signature problem",
};

export function SignatureBanner({ reports, dismissed, onDismiss }: Props) {
  const [expanded, setExpanded] = useState(false);
  const summary = summarise(reports);
  if (!summary || dismissed) return null;

  return (
    <div
      role="status"
      aria-label="Signature status"
      className={`flex flex-col gap-1 border-b px-3 py-1.5 text-xs ${TONE[summary.severity]}`}
    >
      <div className="flex items-center gap-2">
        <span className="font-medium">{LABEL[summary.severity]}</span>
        <span>{summary.headline}</span>
        <button
          type="button"
          onClick={() => setExpanded((e) => !e)}
          className="ml-auto rounded border border-current/30 px-1.5 py-0.5"
        >
          {expanded ? "Hide details" : "Details"}
        </button>
        <button
          type="button"
          onClick={onDismiss}
          aria-label="Dismiss signature status"
          className="rounded border border-current/30 px-1.5 py-0.5"
        >
          Dismiss
        </button>
      </div>

      {expanded ? (
        <ul className="flex flex-col gap-1.5 pt-1">
          {reports.map((report, i) => {
            const status = describeSignature(report);
            return (
              <li key={report.fieldName ?? i} className="border-t border-current/20 pt-1.5">
                <div className="font-medium">{status.headline}</div>
                {report.signedAt ? (
                  <div className="opacity-80">Claimed signing time: {report.signedAt}</div>
                ) : null}
                {report.reason ? <div className="opacity-80">Reason: {report.reason}</div> : null}
                <ul className="list-disc pl-4 opacity-80">
                  {status.notes.map((note) => (
                    <li key={note}>{note}</li>
                  ))}
                </ul>
              </li>
            );
          })}
        </ul>
      ) : null}
    </div>
  );
}
