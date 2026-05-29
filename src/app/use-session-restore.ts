// SPEC: P1-VIEW-011 (+ P1-VIEW-001 CLI clause) — startup session
// restore and continuous persistence, extracted from App.tsx.
//
// On mount: load the saved session, silently re-open each path, then
// drain any CLI-pending paths (routed through `openByPath` so they get
// the same password / recents / persist treatment). After that, persist
// the open tab set + active tab on every change.

import { useEffect, useRef } from "react";

import { takePendingCliOpens } from "@/ipc/cli";
import { openPdfPath } from "@/ipc/pdf";
import { loadSession, saveSession } from "@/ipc/session";
import { useDocumentStore } from "@/state/document-store";

// Session restore must happen exactly once per app launch, and the
// persist gate must outlive a StrictMode remount.
//
// React 18 StrictMode double-invokes effects in dev (mount → unmount →
// remount with a *fresh* component instance). A per-instance `useRef`
// gate would be reset to `false` on the remount and never set back, so
// the persist effect would early-return forever. Both flags therefore
// live at module scope:
//   - `started`  — the restore IIFE runs at most once (no double-open /
//     orphaned actors).
//   - `finished` — gates the persist effect so it can't fire with the
//     initial empty `docs` and wipe session.json before restore loads.
let sessionRestoreStarted = false;
let sessionRestoreFinished = false;

/**
 * @param openByPath the app's single open-orchestrator, used to drain
 *   CLI-pending paths. Passed in (rather than imported) so CLI opens
 *   get the same password-prompt / recents / persist treatment as any
 *   other open.
 */
export function useSessionRestore(
  openByPath: (path: string) => Promise<void>,
): void {
  const docs = useDocumentStore((s) => s.docs);
  const currentId = useDocumentStore((s) => s.currentId);
  const restoreDocs = useDocumentStore((s) => s.restoreDocs);

  // `openByPath` is defined by `useFileOpen` and changes identity across
  // renders; stash the latest in a ref so the restore IIFE (which runs
  // once) always calls the current one without depending on its
  // identity. Render-time assignment is the standard "latest closure"
  // pattern and needs no effect.
  const openByPathRef = useRef(openByPath);
  openByPathRef.current = openByPath;

  // SPEC: P1-VIEW-011 — session restore on mount.
  useEffect(() => {
    if (sessionRestoreStarted) return;
    sessionRestoreStarted = true;
    void (async () => {
      try {
        const session = await loadSession();
        // Open each saved path silently (no password prompt at launch).
        // Encrypted / missing / moved files are skipped; the user
        // re-opens them from recents to get the prompt.
        const opened = await Promise.all(
          session.open.map(async (path) => {
            try {
              return await openPdfPath(path);
            } catch {
              return null;
            }
          }),
        );
        const restored = opened.filter((d) => d !== null);
        restoreDocs(restored, session.active);
        // Open the persistence gate before the CLI drain so each CLI
        // open is captured by the persist effect (otherwise the gate
        // would still be closed and we'd save a session missing the
        // CLI-added tabs).
        sessionRestoreFinished = true;

        // SPEC: P1-VIEW-001 (CLI-arg clause) — drain the buffer Rust
        // parsed from argv in `setup`. Route through `openByPath` (via
        // the ref) so CLI files get the same password prompt + recents
        // + session-persist treatment as any other open. The backend
        // mem::takes the buffer on first call, so this is naturally
        // once-only even if something retriggered it.
        const cliPaths = await takePendingCliOpens();
        for (const path of cliPaths) {
          await openByPathRef.current(path);
        }
      } catch (err) {
        console.warn("session restore failed", err);
        // Open the persistence gate even if restore failed — from here
        // on, user actions should be saved.
        sessionRestoreFinished = true;
      }
    })();
  }, [restoreDocs]);

  // SPEC: P1-VIEW-011 — persist the open tabs + active tab on every
  // change, once restore has run. Backend write is atomic.
  useEffect(() => {
    if (!sessionRestoreFinished) return;
    const activePath = docs.find((d) => d.id === currentId)?.path ?? null;
    void saveSession(
      docs.map((d) => d.path),
      activePath,
    );
  }, [docs, currentId]);
}
