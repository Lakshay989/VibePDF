// SPEC: P1-VIEW-001 — open via drag-drop (step P1.A1).
// SPEC: P1-VIEW-003 — encrypted PDFs dropped onto the window route
// through the same password-prompt flow as the Cmd/Ctrl+O path.
//
// Pure path-filter logic is exported separately from the Tauri-bound
// listener so it can be tested without spinning up a webview.
import { getCurrentWebview } from "@tauri-apps/api/webview";

import {
  openWithPasswordPrompt,
  type AskForPassword,
} from "@/app/open-with-password";
import { openPdfPath, type OpenedDocument } from "@/ipc/pdf";

export interface DragDropResult {
  opened: OpenedDocument[];
  /** Non-PDF files **and** PDFs whose open ultimately failed (bad file, cancelled password prompt, exhausted attempts). */
  rejected: string[];
}

export function isPdfPath(path: string): boolean {
  return path.toLowerCase().endsWith(".pdf");
}

export function partitionPaths(paths: readonly string[]): {
  pdfs: string[];
  others: string[];
} {
  const pdfs: string[] = [];
  const others: string[] = [];
  for (const p of paths) {
    if (isPdfPath(p)) pdfs.push(p);
    else others.push(p);
  }
  return { pdfs, others };
}

/**
 * `askForPassword` is optional — when omitted, an encrypted PDF
 * silently lands in `rejected`, preserving the pre-B2 behaviour for
 * tests and any caller that has no UI to prompt with. App.tsx supplies
 * the real callback in production.
 */
export async function handleDroppedPaths(
  paths: readonly string[],
  askForPassword?: AskForPassword,
): Promise<DragDropResult> {
  const { pdfs, others } = partitionPaths(paths);
  const opened: OpenedDocument[] = [];
  for (const path of pdfs) {
    try {
      if (askForPassword) {
        const result = await openWithPasswordPrompt(path, askForPassword);
        if (result.outcome === "opened") opened.push(result.doc);
        else others.push(path); // cancelled or failed → treat as rejected
      } else {
        opened.push(await openPdfPath(path));
      }
    } catch (err) {
      others.push(path);
      console.warn("drag-drop: failed to open", path, err);
    }
  }
  return { opened, rejected: others };
}

export async function registerDragDrop(
  onDrop: (result: DragDropResult) => void,
  askForPassword?: AskForPassword,
): Promise<() => void> {
  const webview = getCurrentWebview();
  return webview.onDragDropEvent((event) => {
    if (event.payload.type !== "drop") return;
    void handleDroppedPaths(event.payload.paths, askForPassword).then(onDrop);
  });
}
