// SPEC: P1-VIEW-001 — open via drag-drop (step P1.A1).
//
// Pure path-filter logic is exported separately from the Tauri-bound
// listener so it can be tested without spinning up a webview.
import { getCurrentWebview } from "@tauri-apps/api/webview";

import { openPdfPath, type OpenedDocument } from "@/ipc/pdf";

export interface DragDropResult {
  opened: OpenedDocument[];
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

export async function handleDroppedPaths(
  paths: readonly string[],
): Promise<DragDropResult> {
  const { pdfs, others } = partitionPaths(paths);
  const opened: OpenedDocument[] = [];
  for (const path of pdfs) {
    try {
      opened.push(await openPdfPath(path));
    } catch (err) {
      others.push(path);
      console.warn("drag-drop: failed to open", path, err);
    }
  }
  return { opened, rejected: others };
}

export async function registerDragDrop(
  onDrop: (result: DragDropResult) => void,
): Promise<() => void> {
  const webview = getCurrentWebview();
  return webview.onDragDropEvent((event) => {
    if (event.payload.type !== "drop") return;
    void handleDroppedPaths(event.payload.paths).then(onDrop);
  });
}
