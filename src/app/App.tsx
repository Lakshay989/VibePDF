import { useCallback } from "react";

import { PasswordPromptDialog } from "@/app/PasswordPromptDialog";
import { RecoveryDialog } from "@/app/RecoveryDialog";
import { Toasts } from "@/app/Toasts";
import { basename } from "@/app/paths";
import { useFileOpen } from "@/app/use-file-open";
import { useHistory } from "@/app/use-history";
import { useNotesSync } from "@/app/use-notes-sync";
import { useRecovery } from "@/app/use-recovery";
import { useSave } from "@/app/use-save";
import { useSessionRestore } from "@/app/use-session-restore";
import { closePdf, type DocumentId } from "@/ipc/pdf";
import { useDocumentStore } from "@/state/document-store";
import { useSettingsStore } from "@/state/settings-store";
import { PdfViewer } from "@/view/PdfViewer";

export function App() {
  const docs = useDocumentStore((s) => s.docs);
  const currentId = useDocumentStore((s) => s.currentId);
  const setCurrent = useDocumentStore((s) => s.setCurrent);
  const closeDoc = useDocumentStore((s) => s.closeDoc);

  // Close a tab: drop it from the UI, then tell the backend to drop the
  // actor (free the document). Idempotent on the backend.
  const closeTab = useCallback(
    async (id: DocumentId) => {
      closeDoc(id);
      try {
        await closePdf(id);
      } catch (err) {
        console.warn("close failed", id, err);
      }
    },
    [closeDoc],
  );

  // SPEC: P1-VIEW-012 — recents for the start screen.
  const recents = useSettingsStore((s) => s.recents);
  const clearRecents = useSettingsStore((s) => s.clearRecents);

  // The "open a file" subsystem (password prompt, toast, drag-drop,
  // Cmd/Ctrl+O, recents hydration) and the session restore + persist
  // lifecycle live in dedicated hooks; App is composition + layout.
  const { openByPath, pickAndOpen, toast, passwordDialogProps } = useFileOpen();
  useSessionRestore(openByPath);

  // SPEC: P2.A2 — offer to recover unsaved changes from a previous run.
  const {
    entries: recoveryEntries,
    recover,
    discard: discardRecovery,
  } = useRecovery(openByPath);

  const current = docs.find((d) => d.id === currentId);

  // SPEC: P2-SAVE-001 — Cmd/Ctrl+S saves the active document.
  const { save, toast: saveToast } = useSave(current?.id);

  // SPEC: P2-PAGE-003 / session history — Cmd/Ctrl+Z, Cmd/Ctrl+Shift+Z.
  const { canUndo, canRedo, undo, redo } = useHistory(current?.id);

  // SPEC: P3-ANN-002 (re-openable) — project the PDF's sticky notes into the
  // overlay on open and re-sync after undo/redo.
  useNotesSync(current?.id);

  // One status line: a save message takes precedence over an open message
  // when both are live (both auto-dismiss in 3s, so overlap is brief).
  const statusToast = saveToast ?? toast;

  return (
    <div className="flex h-screen flex-col">
      <header className="flex items-center gap-3 border-b border-neutral-200 px-3 py-2 dark:border-neutral-800">
        <div className="font-semibold">VibePDF</div>
        <div className="flex gap-1 overflow-x-auto">
          {docs.map((d) => (
            <div
              key={d.id}
              className={
                "flex items-center rounded text-sm " +
                (d.id === currentId
                  ? "bg-neutral-200 dark:bg-neutral-800"
                  : "hover:bg-neutral-100 dark:hover:bg-neutral-900")
              }
              title={d.path}
            >
              <button
                onClick={() => setCurrent(d.id)}
                className="max-w-[180px] truncate py-1 pl-2 pr-1"
              >
                {d.name}
              </button>
              <button
                onClick={() => void closeTab(d.id)}
                aria-label={`Close ${d.name}`}
                title="Close"
                className="rounded-r px-1.5 py-1 text-neutral-400 hover:text-neutral-700 dark:hover:text-neutral-200"
              >
                ×
              </button>
            </div>
          ))}
        </div>
        <button
          onClick={() => void pickAndOpen()}
          className="ml-auto rounded border border-neutral-300 px-2 py-1 text-sm hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-900"
        >
          Open PDF (⌘O)
        </button>
        <button
          onClick={() => void save()}
          disabled={!current}
          className="rounded border border-neutral-300 px-2 py-1 text-sm hover:bg-neutral-100 disabled:cursor-not-allowed disabled:opacity-40 dark:border-neutral-700 dark:hover:bg-neutral-900"
        >
          Save (⌘S)
        </button>
        <button
          onClick={() => void undo()}
          disabled={!canUndo}
          title="Undo (⌘Z)"
          aria-label="Undo"
          className="rounded border border-neutral-300 px-2 py-1 text-sm hover:bg-neutral-100 disabled:cursor-not-allowed disabled:opacity-40 dark:border-neutral-700 dark:hover:bg-neutral-900"
        >
          ↶
        </button>
        <button
          onClick={() => void redo()}
          disabled={!canRedo}
          title="Redo (⌘⇧Z)"
          aria-label="Redo"
          className="rounded border border-neutral-300 px-2 py-1 text-sm hover:bg-neutral-100 disabled:cursor-not-allowed disabled:opacity-40 dark:border-neutral-700 dark:hover:bg-neutral-900"
        >
          ↷
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
      {statusToast ? (
        <div
          role="status"
          className="pointer-events-none fixed bottom-4 left-1/2 -translate-x-1/2 rounded bg-neutral-900/90 px-3 py-2 text-sm text-white shadow-lg dark:bg-neutral-100/90 dark:text-neutral-900"
        >
          {statusToast}
        </div>
      ) : null}
      <PasswordPromptDialog {...passwordDialogProps} />
      <RecoveryDialog
        entries={recoveryEntries}
        onRecover={(e) => void recover(e)}
        onDiscard={(e) => void discardRecovery(e)}
      />
      <Toasts />
    </div>
  );
}

interface EmptyStateProps {
  recents: string[];
  onOpenRecent: (path: string) => void;
  onClearRecents: () => void;
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
