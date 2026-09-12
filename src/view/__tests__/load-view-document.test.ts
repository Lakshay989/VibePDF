// SPEC: P1-VIEW-003 — the route an encrypted document takes to the screen.
//
// The failure this pins was invisible to every existing test: the backend
// opened encrypted files fine, and the view then handed PDF.js bytes it could
// not open without a password it never has.

import { describe, expect, it, vi } from "vitest";
import type { PDFDocumentProxy } from "pdfjs-dist";

import type { DocumentId } from "@/ipc/pdf";
import { loadViewDocument, type ViewDocumentSources } from "@/view/load-view-document";

const ID = "doc-1" as DocumentId;
const PATH = "/docs/protected.pdf";
const DISK = new Uint8Array([1]);
const ACTOR = new Uint8Array([2]);
const DOC = { numPages: 1 } as unknown as PDFDocumentProxy;

function pdfjsError(name: string, message: string): Error {
  const e = new Error(message);
  e.name = name;
  return e;
}

function sources(load: ViewDocumentSources["loadDocument"]) {
  return {
    readFile: vi.fn<ViewDocumentSources["readFile"]>(() => Promise.resolve(DISK)),
    getPdfBytes: vi.fn<ViewDocumentSources["getPdfBytes"]>(() => Promise.resolve(ACTOR)),
    loadDocument: vi.fn<ViewDocumentSources["loadDocument"]>(load),
  };
}

describe("loadViewDocument", () => {
  it("loads a pristine document from disk", async () => {
    const src = sources(() => Promise.resolve(DOC));
    await expect(loadViewDocument({ documentId: ID, path: PATH, edited: false }, src)).resolves.toBe(DOC);
    expect(src.readFile).toHaveBeenCalledWith(PATH);
    expect(src.loadDocument).toHaveBeenCalledWith(DISK);
    expect(src.getPdfBytes).not.toHaveBeenCalled();
  });

  it("loads an edited document from the actor's live bytes", async () => {
    const src = sources(() => Promise.resolve(DOC));
    await loadViewDocument({ documentId: ID, path: PATH, edited: true }, src);
    expect(src.getPdfBytes).toHaveBeenCalledWith(ID);
    expect(src.readFile).not.toHaveBeenCalled();
  });

  it("gets an encrypted document from the actor instead of needing its password", async () => {
    const src = sources((bytes) =>
      bytes === DISK
        ? Promise.reject(pdfjsError("PasswordException", "No password given"))
        : Promise.resolve(DOC),
    );
    await expect(loadViewDocument({ documentId: ID, path: PATH, edited: false }, src)).resolves.toBe(DOC);
    expect(src.getPdfBytes).toHaveBeenCalledTimes(1);
    expect(src.loadDocument.mock.calls.map(([b]) => b)).toEqual([DISK, ACTOR]);
  });

  // A broken file retried against the actor would fail again with a less
  // truthful message — or worse, succeed and hide that the file on disk is bad.
  it("does not reroute a document that is simply not a PDF", async () => {
    const src = sources(() => Promise.reject(pdfjsError("InvalidPDFException", "Invalid PDF structure.")));
    await expect(loadViewDocument({ documentId: ID, path: PATH, edited: false }, src)).rejects.toThrow(
      "Invalid PDF structure.",
    );
    expect(src.getPdfBytes).not.toHaveBeenCalled();
  });
});
