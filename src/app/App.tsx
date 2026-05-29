import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { useCallback, useEffect, useRef, useState } from "react";

import { PasswordPromptDialog } from "@/app/PasswordPromptDialog";
import { registerDragDrop } from "@/app/drag-drop";
import {
  openWithPasswordPrompt,
  type AskForPassword,
  type PasswordPromptRequest,
} from "@/app/open-with-password";
import { takePendingCliOpens } from "@/ipc/cli";
import { openPdfPath } from "@/ipc/pdf";
import { loadSession, saveSession } from "@/ipc/session";
import { useDocumentStore } from "@/state/document-store";
import { useSettingsStore } from "@/state/settings-store";
import { PdfViewer } from "@/view/PdfViewer";

// SPEC: P1-VIEW-011 — session restore must happen exactly once per app
// launch, and the persist gate must outlive a StrictMode remount.
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

export function App() {
  const docs = useDocumentStore((s) => s.docs);
  const currentId = useDocumentStore((s) => s.currentId);
  const openDoc = useDocumentStore((s) => s.openDoc);
  const setCurrent = useDocumentStore((s) => s.setCurrent);
  const restoreDocs = useDocumentStore((s) => s.restoreDocs);
  const [toast, setToast] = useState<string | null>(null);

  // SPEC: P1-VIEW-012 — recents, owned by Rust and mirrored here.
  const recents = useSettingsStore((s) => s.recents);
  const hydrateRecents = useSettingsStore((s) => s.hydrateRecents);
  const pushRecent = useSettingsStore((s) => s.pushRecent);
  const clearRecents = useSettingsStore((s) => s.clearRecents);

  useEffect(() => {
    void hydrateRecents();
  }, [hydrateRecents]);

  // SPEC: P1-VIEW-001 (CLI-arg clause) — `openByPath` is defined later
  // in the body but the session-restore IIFE below needs to call it
  // when draining pending CLI paths. The render-time ref assignment
  // (further down) always points at the current useCallback, so the
  // closure here gets the latest `openByPath` without retriggering the
  // IIFE on identity changes.
  const openByPathRef = useRef<((path: string) => Promise<void>) | null>(null);

  // SPEC: P1-VIEW-011 — session restore on mount. See the module-level
  // flags above for why the gate lives outside the component.
  useEffect(() => {
    if (sessionRestoreStarted) return;
    sessionRestoreStarted = true;
    void (async () => {
      try {
        const session = await loadSession();
        // Open each saved path silently (no password prompt at launch —
        // see the plan's risk #2). Encrypted / missing / moved files are
        // skipped; the user re-opens them from recents to get the prompt.
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
          await openByPathRef.current?.(path);
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

  // SPEC: P1-VIEW-003 — password-prompt state.
  // `prompt` non-null means the dialog is mounted with these args.
  // `resolveRef` carries the in-flight `Promise<string | null>`'s
  // resolver so the dialog buttons can settle the awaiting retry loop.
  const [prompt, setPrompt] = useState<PasswordPromptRequest | null>(null);
  const resolveRef = useRef<((pwd: string | null) => void) | null>(null);

  const askForPassword: AskForPassword = useCallback(
    (req) =>
      new Promise<string | null>((resolve) => {
        resolveRef.current = resolve;
        setPrompt(req);
      }),
    [],
  );

  const handleDialogSubmit = useCallback((pwd: string) => {
    // The retry loop will either succeed (we'll clear `prompt` below)
    // or re-prompt with new args (which replaces `prompt` and clears
    // the dialog input via PasswordPromptDialog's effect).
    resolveRef.current?.(pwd);
    resolveRef.current = null;
  }, []);

  const handleDialogCancel = useCallback(() => {
    resolveRef.current?.(null);
    resolveRef.current = null;
    setPrompt(null);
  }, []);

  // SPEC: P1-VIEW-001 + P1-VIEW-003 — single entry point for any
  // path-driven open. The Cmd/Ctrl+O path, the header button, and the
  // drag-drop callback all converge here. Encrypted opens retry up to
  // 3 times via `openWithPasswordPrompt`; terminal failure surfaces as
  // a toast.
  const openByPath = useCallback(
    async (path: string) => {
      try {
        const result = await openWithPasswordPrompt(path, askForPassword);
        switch (result.outcome) {
          case "opened":
            openDoc(result.doc);
            setPrompt(null);
            // SPEC: P1-VIEW-012 — only successful opens count as recent.
            void pushRecent(result.doc.path);
            break;
          case "cancelled":
            // User dismissed the dialog. handleDialogCancel already
            // cleared `prompt`. No toast — explicit user action.
            break;
          case "failed":
            setPrompt(null);
            setToast("Could not unlock.");
            break;
        }
      } catch (err) {
        // Non-password errors (NotFound, PdfError, etc.). Keep the
        // pre-B2 behaviour: log + best-effort toast.
        setPrompt(null);
        console.warn("openByPath failed", path, err);
        setToast(err instanceof Error ? err.message : "Could not open file.");
      }
    },
    [askForPassword, openDoc, pushRecent],
  );

  // Keep the restore-IIFE's CLI drain hook (see ref declaration above)
  // pointed at the latest `openByPath`. Render-time ref assignment is
  // the standard "latest closure" pattern and doesn't need a useEffect.
  openByPathRef.current = openByPath;

  const pickAndOpen = useCallback(async () => {
    const selected = await openFileDialog({
      multiple: false,
      filters: [{ name: "PDF", extensions: ["pdf"] }],
    });
    if (selected && typeof selected === "string") {
      await openByPath(selected);
    }
  }, [openByPath]);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const cmd = e.metaKey || e.ctrlKey;
      if (cmd && e.key.toLowerCase() === "o") {
        e.preventDefault();
        void pickAndOpen();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [pickAndOpen]);

  // SPEC: P1-VIEW-001 (P1.A1) — drag-drop file open.
  // SPEC: P1-VIEW-003 — encrypted drops route through the same prompt.
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    void registerDragDrop(
      ({ opened, rejected }) => {
        for (const doc of opened) {
          openDoc(doc);
          // SPEC: P1-VIEW-012 — dropped opens count as recent too.
          void pushRecent(doc.path);
        }
        if (rejected.length > 0) {
          setToast(
            rejected.length === 1
              ? "Only .pdf files are accepted."
              : `${rejected.length} files were ignored — only .pdf is accepted.`,
          );
        }
      },
      askForPassword,
    ).then((u) => {
      unlisten = u;
    });
    return () => {
      unlisten?.();
    };
  }, [openDoc, askForPassword, pushRecent]);

  useEffect(() => {
    if (!toast) return;
    const id = window.setTimeout(() => setToast(null), 3000);
    return () => window.clearTimeout(id);
  }, [toast]);

  const current = docs.find((d) => d.id === currentId);

  return (
    <div className="flex h-screen flex-col">
      <header className="flex items-center gap-3 border-b border-neutral-200 px-3 py-2 dark:border-neutral-800">
        <div className="font-semibold">VibePDF</div>
        <div className="flex gap-1 overflow-x-auto">
          {docs.map((d) => (
            <button
              key={d.id}
              onClick={() => setCurrent(d.id)}
              className={
                "max-w-[200px] truncate rounded px-2 py-1 text-sm " +
                (d.id === currentId
                  ? "bg-neutral-200 dark:bg-neutral-800"
                  : "hover:bg-neutral-100 dark:hover:bg-neutral-900")
              }
              title={d.path}
            >
              {d.name}
            </button>
          ))}
        </div>
        <button
          onClick={() => void pickAndOpen()}
          className="ml-auto rounded border border-neutral-300 px-2 py-1 text-sm hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-900"
        >
          Open PDF (⌘O)
        </button>
      </header>
      <main className="flex-1 overflow-hidden">
        {current ? (
          <PdfViewer documentId={current.id} path={current.path} />
        ) : (
          <EmptyState
            recents={recents}
            onOpenRecent={(path) => void openByPath(path)}
            onClearRecents={() => void clearRecents()}
          />
        )}
      </main>
      {toast ? (
        <div
          role="status"
          className="pointer-events-none fixed bottom-4 left-1/2 -translate-x-1/2 rounded bg-neutral-900/90 px-3 py-2 text-sm text-white shadow-lg dark:bg-neutral-100/90 dark:text-neutral-900"
        >
          {toast}
        </div>
      ) : null}
      <PasswordPromptDialog
        request={prompt}
        onSubmit={handleDialogSubmit}
        onCancel={handleDialogCancel}
      />
    </div>
  );
}

interface EmptyStateProps {
  recents: string[];
  onOpenRecent: (path: string) => void;
  onClearRecents: () => void;
}

function basename(path: string): string {
  const sep = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return sep >= 0 ? path.slice(sep + 1) : path;
}

// SPEC: P1-VIEW-012 — recents surface on the start screen, clearable.
function EmptyState({ recents, onOpenRecent, onClearRecents }: EmptyStateProps) {
  return (
    <div className="flex h-full items-center justify-center text-neutral-500">
      <div className="w-[420px] max-w-[90%] text-center">
        <div className="text-lg">No document open</div>
        <div className="mt-1 text-sm">Press ⌘O / Ctrl+O to open a PDF.</div>

        {recents.length > 0 ? (
          <div className="mt-6 text-left">
            <div className="mb-1 flex items-center justify-between px-1">
              <span className="text-xs font-medium uppercase tracking-wide text-neutral-400">
                Recent
              </span>
              <button
                onClick={onClearRecents}
                className="text-xs text-neutral-400 hover:text-neutral-600 hover:underline dark:hover:text-neutral-300"
              >
                Clear recents
              </button>
            </div>
            <ul className="overflow-hidden rounded border border-neutral-200 dark:border-neutral-800">
              {recents.map((path) => (
                <li key={path}>
                  <button
                    onClick={() => onOpenRecent(path)}
                    title={path}
                    className="block w-full truncate px-3 py-2 text-left text-sm hover:bg-neutral-100 dark:hover:bg-neutral-900"
                  >
                    <span className="text-neutral-700 dark:text-neutral-200">
                      {basename(path)}
                    </span>
                    <span className="ml-2 truncate text-xs text-neutral-400">
                      {path}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          </div>
        ) : null}
      </div>
    </div>
  );
}
