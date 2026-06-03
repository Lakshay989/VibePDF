// SPEC: P2-SAVE-001 — explicit save (Cmd/Ctrl+S). Mirrors the Cmd/Ctrl+O
// keydown wiring in use-file-open.ts: the binding lives in a hook, not in
// the App component.

import { useCallback, useEffect, useState } from "react";

import type { DocumentId } from "@/ipc/pdf";
import { savePdf } from "@/ipc/save";

export interface UseSave {
  /** Save the active document to its own path. */
  save: () => Promise<void>;
  /** Transient status message; `null` when nothing is shown. */
  toast: string | null;
}

export function useSave(documentId: DocumentId | undefined): UseSave {
  const [toast, setToast] = useState<string | null>(null);

  const save = useCallback(async () => {
    if (!documentId) return;
    try {
      const outcome = await savePdf(documentId);
      setToast(outcome.noOp ? "No changes to save" : "Saved");
    } catch (err) {
      // A failed save must never look like a success. Surface the
      // typed error message; the original file is untouched (the write
      // path verifies a temp copy before it can replace anything).
      console.warn("save failed", documentId, err);
      setToast(err instanceof Error ? err.message : "Could not save file.");
    }
  }, [documentId]);

  // Cmd/Ctrl+S → save the active document.
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const cmd = e.metaKey || e.ctrlKey;
      if (cmd && e.key.toLowerCase() === "s") {
        e.preventDefault();
        void save();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [save]);

  // Auto-dismiss the toast (matches use-file-open.ts's 3s).
  useEffect(() => {
    if (!toast) return;
    const id = window.setTimeout(() => setToast(null), 3000);
    return () => window.clearTimeout(id);
  }, [toast]);

  return { save, toast };
}
