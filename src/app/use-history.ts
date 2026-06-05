// SPEC: P2-PAGE-003 / session history — undo/redo (Cmd/Ctrl+Z,
// Cmd/Ctrl+Shift+Z). Mirrors the keydown-in-a-hook pattern of
// use-save.ts / use-file-open.ts. The actor owns the real stacks; this
// hook drives the keybindings and mirrors availability into the store.

import { useCallback, useEffect } from "react";

import {
  historyState,
  redo as redoCmd,
  undo as undoCmd,
} from "@/ipc/history";
import type { DocumentId } from "@/ipc/pdf";
import { useDocHistory, useHistoryStore } from "@/state/history-store";

export interface UseHistory {
  canUndo: boolean;
  canRedo: boolean;
  undo: () => Promise<void>;
  redo: () => Promise<void>;
}

export function useHistory(documentId: DocumentId | undefined): UseHistory {
  const setHistory = useHistoryStore((s) => s.setHistory);
  const { canUndo, canRedo } = useDocHistory(documentId);

  // Hydrate availability when the active document changes. For a freshly
  // opened doc this is {false, false}; an autosave-restored doc (P2.A2)
  // or a doc edited then switched away from could differ.
  useEffect(() => {
    if (!documentId) return;
    let cancelled = false;
    void historyState(documentId)
      .then((s) => {
        if (!cancelled) setHistory(documentId, s);
      })
      .catch((err) => console.warn("history_state failed", documentId, err));
    return () => {
      cancelled = true;
    };
  }, [documentId, setHistory]);

  const undo = useCallback(async () => {
    if (!documentId) return;
    try {
      setHistory(documentId, await undoCmd(documentId));
    } catch (err) {
      console.warn("undo failed", documentId, err);
    }
  }, [documentId, setHistory]);

  const redo = useCallback(async () => {
    if (!documentId) return;
    try {
      setHistory(documentId, await redoCmd(documentId));
    } catch (err) {
      console.warn("redo failed", documentId, err);
    }
  }, [documentId, setHistory]);

  // Cmd/Ctrl+Z undo; Cmd/Ctrl+Shift+Z redo.
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const cmd = e.metaKey || e.ctrlKey;
      if (cmd && e.key.toLowerCase() === "z") {
        e.preventDefault();
        if (e.shiftKey) void redo();
        else void undo();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [undo, redo]);

  return { canUndo, canRedo, undo, redo };
}
