// SPEC: P5-FORM-001 (P5.A1) — detect the open document's AcroForm, and reset
// form state when the document changes. Populates `useFormStore.detected` so the
// header can surface a "Form mode" entry point with the field count.

import { useEffect } from "react";

import { readFormSummary } from "@/ipc/forms";
import type { DocumentId } from "@/ipc/pdf";
import { useDocEpoch } from "@/state/edit-epoch-store";
import { useFormStore } from "@/state/form-store";

export function useFormDetect(documentId: DocumentId | undefined): void {
  const setDetected = useFormStore((s) => s.setDetected);
  const exitFormMode = useFormStore((s) => s.exitFormMode);
  const epoch = useDocEpoch(documentId ?? "");

  // Switching documents clears both detection and form mode. Keyed on the
  // document alone — an edit must NOT knock the user out of form mode.
  useEffect(() => {
    setDetected(null);
    exitFormMode();
  }, [documentId, setDetected, exitFormMode]);

  // Re-read the summary on every edit epoch, not just on open. A1 adds no
  // fields, but B1/B2 create them and B3 deletes them — and crucially undo/redo
  // changes the count without going through the panel that used to be the only
  // thing refreshing it, so the header sat stale after ⌘Z (P5 sweep B3).
  useEffect(() => {
    if (!documentId) return;
    let cancelled = false;
    void readFormSummary(documentId)
      .then((summary) => {
        if (!cancelled) setDetected(summary);
      })
      .catch((err: unknown) => {
        // A failed detection must never block the viewer — just no entry point.
        console.warn("form detect failed", documentId, err);
      });
    return () => {
      cancelled = true;
    };
  }, [documentId, epoch, setDetected]);
}
