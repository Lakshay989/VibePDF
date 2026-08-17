// SPEC: P6-SEC-006 (P6.B2b) — "WHEN the user opens a signed PDF, THE system
// SHALL verify all signatures".
//
// On open, not on request: a reader who has to go looking for the signature
// panel is a reader who will not look. Keyed on the document id alone — the
// backend reports on the file *as saved*, so in-session edits change nothing it
// would see.

import { useEffect, useState } from "react";

import type { DocumentId, SignatureReport } from "@/ipc/pdf";
import { verifySignatures } from "@/ipc/pdf";

export interface UseSignatures {
  /** One entry per signature; empty for the overwhelming majority of files. */
  reports: SignatureReport[];
  dismissed: boolean;
  dismiss: () => void;
}

export function useSignatures(documentId: DocumentId | undefined): UseSignatures {
  const [reports, setReports] = useState<SignatureReport[]>([]);
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => {
    setReports([]);
    setDismissed(false);
    if (!documentId) return;
    let cancelled = false;
    void verifySignatures(documentId)
      .then((r) => {
        if (!cancelled) setReports(r);
      })
      .catch((err: unknown) => {
        // A document whose signatures cannot be read is still a document, and
        // failing to check them must never stop it opening.
        console.warn("signature verification failed", documentId, err);
      });
    return () => {
      cancelled = true;
    };
  }, [documentId]);

  return { reports, dismissed, dismiss: () => setDismissed(true) };
}
