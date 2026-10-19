// SPEC: P1-VIEW-003 — which bytes PDF.js renders.
//
// A pristine document loads straight from disk (cheap); an edited one — even a
// rotate, which doesn't bump the epoch — loads from the actor's live bytes,
// which carry the in-memory edits.
//
// An *encrypted* pristine document cannot load from disk at all. PDF.js needs
// its password, and the view layer never has one: the open password lives only
// in the prompt's closure (`app/open-with-password.ts`). The actor serves those
// bytes with the encryption removed, in memory, so they take the same route as
// an edit. Before this, every encrypted PDF opened to "No password given".

import type { PDFDocumentProxy } from "pdfjs-dist";

import type { DocumentId } from "@/ipc/pdf";

export interface ViewDocumentSources {
  readFile: (path: string) => Promise<Uint8Array>;
  getPdfBytes: (id: DocumentId) => Promise<Uint8Array>;
  loadDocument: (bytes: Uint8Array) => Promise<PDFDocumentProxy>;
}

export interface ViewDocumentRequest {
  documentId: DocumentId;
  path: string;
  edited: boolean;
}

export async function loadViewDocument(
  request: ViewDocumentRequest,
  sources: ViewDocumentSources,
): Promise<PDFDocumentProxy> {
  if (request.edited) {
    return sources.loadDocument(await sources.getPdfBytes(request.documentId));
  }
  try {
    return await sources.loadDocument(await sources.readFile(request.path));
  } catch (err) {
    // Only a password failure changes route. Anything else — a file that is
    // simply not a PDF — must surface, not be retried against the actor and
    // reported as something else.
    if (!isPasswordException(err)) throw err;
    return sources.loadDocument(await sources.getPdfBytes(request.documentId));
  }
}

function isPasswordException(err: unknown): boolean {
  return err instanceof Error && err.name === "PasswordException";
}
