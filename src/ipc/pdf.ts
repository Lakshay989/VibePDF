import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@/ipc/invoke";

export type DocumentId = string;

export interface OpenedDocument {
  id: DocumentId;
  path: string;
  name: string;
  pageCount: number;
  title: string | null;
  author: string | null;
  pdfVersion: string | null;
}

/**
 * Payload of the `document-changed` Tauri event. Mirrors
 * `pdf::actor::DocumentChange` on the Rust side.
 *
 * Discriminated by `kind`: the frontend should pattern-match and
 * never assume fields outside the matched variant.
 */
export type DocumentChange =
  | { kind: "opened"; id: DocumentId; pageCount: number }
  | { kind: "closed"; id: DocumentId };

// Wrapper for the dialog → backend "open" flow. Returns null when the
// user dismisses the dialog.
export async function openPdfDialog(): Promise<OpenedDocument | null> {
  const selected = await open({
    multiple: false,
    filters: [{ name: "PDF", extensions: ["pdf"] }],
  });
  if (!selected || typeof selected !== "string") return null;
  return openPdfPath(selected);
}

export async function openPdfPath(path: string): Promise<OpenedDocument> {
  return invoke<OpenedDocument>("pdf_open", { path });
}

export async function closePdf(id: DocumentId): Promise<void> {
  return invoke<void>("pdf_close", { id });
}

export async function pdfiumVersion(): Promise<string> {
  return invoke<string>("pdfium_version");
}
