import { GlobalWorkerOptions } from "pdfjs-dist";

// PDF.js v5 ships the worker as `pdf.worker.min.mjs`. We host it as a
// static asset under /public so Tauri's custom protocol serves it with
// the right MIME type. Re-bundling the worker through Vite is fragile
// when wrapped in Tauri's webview — keeping it as a sibling URL avoids
// the entire class of "worker fails to load over tauri://" bugs.
//
// SPEC: P1-VIEW-001 — the worker must be reachable for any page render.

let configured = false;

export function configurePdfJsWorker(): void {
  if (configured) return;
  GlobalWorkerOptions.workerSrc = new URL(
    "/pdfjs/pdf.worker.min.mjs",
    window.location.origin,
  ).toString();
  configured = true;
}
