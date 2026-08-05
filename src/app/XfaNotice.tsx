// SPEC: P5-FORM-005 (P5.A5) — the XFA-only warning + convert offer.
//
// VibePDF can't render/edit XFA (dynamic) forms. WHERE a document is XFA-only —
// an /XFA layer with no natively-fillable AcroForm fields (field count 0) — this
// banner says editing isn't supported and offers to strip the dynamic layer,
// leaving the static content as a read-only form. Detection comes from the P5.A1
// form summary (useFormStore); the convert action is undoable (stripXfa).

import { reportError } from "@/app/report-error";
import { useState } from "react";

import { readFormSummary, stripXfa } from "@/ipc/forms";
import { useEditEpochStore } from "@/state/edit-epoch-store";
import { useFormStore } from "@/state/form-store";
import { useHistoryStore } from "@/state/history-store";
import { useToastStore } from "@/state/toast-store";

interface Props {
  documentId: string;
}

export function XfaNotice({ documentId }: Props) {
  const detected = useFormStore((s) => s.detected);
  const setDetected = useFormStore((s) => s.setDetected);
  const setHistory = useHistoryStore((s) => s.setHistory);
  const bumpEpoch = useEditEpochStore((s) => s.bumpEpoch);
  const pushToast = useToastStore((s) => s.push);
  const [busy, setBusy] = useState(false);

  // XFA-only = an XFA layer with no natively-fillable AcroForm fields. A hybrid
  // form (fields > 0) keeps Form mode (A2–A4) and shows no warning.
  if (!detected || !detected.hasXfa || detected.fieldCount > 0) return null;

  const convert = () => {
    setBusy(true);
    stripXfa(documentId)
      .then((h) => {
        setHistory(documentId, h);
        bumpEpoch(documentId); // hard reload — the document changed
        pushToast("info", "Converted to a static, read-only form (XFA layer removed).");
        return readFormSummary(documentId).then(setDetected);
      })
      .catch((err: unknown) => reportError("Couldn't convert the XFA form", err))
      .finally(() => setBusy(false));
  };

  return (
    <div
      role="status"
      className="flex items-start gap-3 border-b border-amber-300 bg-amber-50 px-4 py-2 text-sm text-amber-900 dark:border-amber-700/60 dark:bg-amber-950/40 dark:text-amber-200"
    >
      <span aria-hidden className="mt-0.5 select-none">
        ⚠️
      </span>
      <div className="min-w-0 flex-1">
        <p>
          This PDF uses an <span className="font-medium">XFA (dynamic) form</span>, which VibePDF
          can&rsquo;t edit.
        </p>
        <p className="mt-0.5 text-amber-800 dark:text-amber-300/90">
          Convert it to a static, read-only form to keep the visible content.
        </p>
      </div>
      <button
        type="button"
        onClick={convert}
        disabled={busy}
        aria-label="Convert XFA to a static read-only form"
        className="shrink-0 rounded border border-amber-300 px-2 py-1 text-xs hover:bg-amber-100 disabled:cursor-not-allowed disabled:opacity-50 dark:border-amber-700/60 dark:hover:bg-amber-900/40"
      >
        {busy ? "Converting…" : "Convert to static form"}
      </button>
    </div>
  );
}
