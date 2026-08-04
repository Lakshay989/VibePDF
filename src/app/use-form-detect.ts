// SPEC: P5-FORM-001 (P5.A1) — detect the open document's AcroForm once, and
// reset form state when the document changes. Populates `useFormStore.detected`
// so the header can surface a "Form mode" entry point with the field count.

import { useEffect } from "react";

import { readFormSummary } from "@/ipc/forms";
import type { DocumentId } from "@/ipc/pdf";
import { useFormStore } from "@/state/form-store";

export function useFormDetect(documentId: DocumentId | undefined): void {
  const setDetected = useFormStore((s) => s.setDetected);
  const exitFormMode = useFormStore((s) => s.exitFormMode);

  // Keyed on the document id only: A1 adds no fields, so detection doesn't
  // change on edits. Switching documents clears both detection and form mode.
  useEffect(() => {
    setDetected(null);
    exitFormMode();
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
  }, [documentId, setDetected, exitFormMode]);
}
