import { PasswordPromptDialog } from "@/app/PasswordPromptDialog";
import { basename } from "@/app/paths";
import { useFileOpen } from "@/app/use-file-open";
import { useSave } from "@/app/use-save";
import { useSessionRestore } from "@/app/use-session-restore";
import { useDocumentStore } from "@/state/document-store";
import { useSettingsStore } from "@/state/settings-store";
import { PdfViewer } from "@/view/PdfViewer";

export function App() {
  const docs = useDocumentStore((s) => s.docs);
  const currentId = useDocumentStore((s) => s.currentId);
  const setCurrent = useDocumentStore((s) => s.setCurrent);

  // SPEC: P1-VIEW-012 — recents for the start screen.
  const recents = useSettingsStore((s) => s.recents);
  const clearRecents = useSettingsStore((s) => s.clearRecents);

  // The "open a file" subsystem (password prompt, toast, drag-drop,
  // Cmd/Ctrl+O, recents hydration) and the session restore + persist
  // lifecycle live in dedicated hooks; App is composition + layout.
  const { openByPath, pickAndOpen, toast, passwordDialogProps } = useFileOpen();
  useSessionRestore(openByPath);

  const current = docs.find((d) => d.id === currentId);

  // SPEC: P2-SAVE-001 — Cmd/Ctrl+S saves the active document.
  const { save, toast: saveToast } = useSave(current?.id);

  // One status line: a save message takes precedence over an open message
  // when both are live (both auto-dismiss in 3s, so overlap is brief).
  const statusToast = saveToast ?? toast;

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
        <button
          onClick={() => void save()}
          disabled={!current}
          className="rounded border border-neutral-300 px-2 py-1 text-sm hover:bg-neutral-100 disabled:cursor-not-allowed disabled:opacity-40 dark:border-neutral-700 dark:hover:bg-neutral-900"
        >
          Save (⌘S)
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
