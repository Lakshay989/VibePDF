import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@/ipc/invoke";

export type DocumentId = string;

export interface OpenedDocument {
  id: DocumentId;
  path: string;
  name: string;
  pageCount: number;
}

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

export async function pdfiumVersion(): Promise<string> {
  return invoke<string>("pdfium_version");
}
