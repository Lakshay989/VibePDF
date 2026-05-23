import { useEffect } from "react";
import { PdfViewer } from "@/view/PdfViewer";
import { useDocumentStore } from "@/state/document-store";
import { openPdfDialog } from "@/ipc/pdf";

export function App() {
  const docs = useDocumentStore((s) => s.docs);
  const currentId = useDocumentStore((s) => s.currentId);
  const openDoc = useDocumentStore((s) => s.openDoc);
  const setCurrent = useDocumentStore((s) => s.setCurrent);

  // Phase 1: drag-and-drop and CLI-arg open are wired through Tauri
  // listeners in a follow-up commit. For the bootstrap we expose a
  // single "Open PDF" entry point so the smoke demo is keyboard-only.
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const cmd = e.metaKey || e.ctrlKey;
      if (cmd && e.key.toLowerCase() === "o") {
        e.preventDefault();
        void openPdfDialog().then((opened) => {
          if (opened) openDoc(opened);
        });
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [openDoc]);

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
          onClick={() =>
            void openPdfDialog().then((opened) => opened && openDoc(opened))
          }
          className="ml-auto rounded border border-neutral-300 px-2 py-1 text-sm hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-900"
        >
          Open PDF (⌘O)
        </button>
      </header>
      <main className="flex-1 overflow-hidden">
        {current ? (
          <PdfViewer documentId={current.id} path={current.path} />
        ) : (
          <EmptyState />
        )}
      </main>
    </div>
  );
}

function EmptyState() {
  return (
    <div className="flex h-full items-center justify-center text-neutral-500">
      <div className="text-center">
        <div className="text-lg">No document open</div>
        <div className="mt-1 text-sm">Press ⌘O / Ctrl+O to open a PDF.</div>
      </div>
    </div>
  );
}
