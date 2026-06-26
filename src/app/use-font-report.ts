// SPEC: P4-EDIT-002 (P4.A2) — the once-per-document font-fallback warning.
// Fetches the font report when the open document changes; the banner shows
// only while `needsFallback` and the user hasn't dismissed it for this doc.

import { useEffect, useState } from "react";

import type { FontReport } from "@/ipc/fonts";
import { readFontReport } from "@/ipc/fonts";
import type { DocumentId } from "@/ipc/pdf";

export interface UseFontReport {
  /** The resolved report, or `null` until it loads (or if it failed). */
  report: FontReport | null;
  /** True once the user dismisses the banner for the current document. */
  dismissed: boolean;
  /** Hide the banner for the current document (the "once" in once-per-doc). */
  dismiss: () => void;
}

export function useFontReport(documentId: DocumentId | undefined): UseFontReport {
  const [report, setReport] = useState<FontReport | null>(null);
  const [dismissed, setDismissed] = useState(false);

  // Re-fetch per document. Font usage doesn't change on annotation edits and
  // A2 adds no text, so this is keyed on the id only — not the edit epoch.
  useEffect(() => {
    setReport(null);
    setDismissed(false);
    if (!documentId) return;
    let cancelled = false;
    void readFontReport(documentId)
      .then((r) => {
        if (!cancelled) setReport(r);
      })
      .catch((err: unknown) => {
        // A failed report must never block the viewer — just no banner.
        console.warn("font report failed", documentId, err);
      });
    return () => {
      cancelled = true;
    };
  }, [documentId]);

  return { report, dismissed, dismiss: () => setDismissed(true) };
}
