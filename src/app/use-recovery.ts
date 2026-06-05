// SPEC: P2.A2 — crash recovery. On startup, list any unsaved-changes
// copies the autosave loop left behind (a previous run that didn't exit
// cleanly) and let the user reopen or discard each.
//
// "Recover" opens the autosave file via the app's normal open path, then
// drops the recovery copy. (Adopting the *original* path so a later ⌘S
// targets it is a noted backlog refinement — for now recovery is safe and
// never clobbers the original.) "Discard" just drops the copy.

import { useCallback, useEffect, useRef, useState } from "react";

import {
  recoveryDiscard,
  recoveryList,
  type RecoveryEntry,
} from "@/ipc/recovery";

// The startup scan must run at most once per launch and survive a
// StrictMode remount, so the guard lives at module scope (same reasoning
// as use-session-restore.ts).
let recoveryScanStarted = false;

export interface UseRecovery {
  /** Documents with a recovery copy still on disk. */
  entries: RecoveryEntry[];
  /** Reopen the autosave copy, then drop it. */
  recover: (entry: RecoveryEntry) => Promise<void>;
  /** Drop the recovery copy without opening it. */
  discard: (entry: RecoveryEntry) => Promise<void>;
}

export function useRecovery(
  openByPath: (path: string) => Promise<void>,
): UseRecovery {
  const [entries, setEntries] = useState<RecoveryEntry[]>([]);

  // `openByPath` changes identity across renders; keep the latest in a ref
  // so the callbacks don't churn (mirrors use-session-restore.ts).
  const openByPathRef = useRef(openByPath);
  openByPathRef.current = openByPath;

  useEffect(() => {
    if (recoveryScanStarted) return;
    recoveryScanStarted = true;
    void recoveryList()
      .then(setEntries)
      .catch((err) => console.warn("recovery_list failed", err));
  }, []);

  const remove = useCallback((id: string) => {
    setEntries((prev) => prev.filter((e) => e.documentId !== id));
  }, []);

  const recover = useCallback(
    async (entry: RecoveryEntry) => {
      remove(entry.documentId);
      try {
        await openByPathRef.current(entry.autosavePath);
        await recoveryDiscard(entry.documentId);
      } catch (err) {
        console.warn("recover failed", entry.documentId, err);
      }
    },
    [remove],
  );

  const discard = useCallback(
    async (entry: RecoveryEntry) => {
      remove(entry.documentId);
      try {
        await recoveryDiscard(entry.documentId);
      } catch (err) {
        console.warn("discard failed", entry.documentId, err);
      }
    },
    [remove],
  );

  return { entries, recover, discard };
}
