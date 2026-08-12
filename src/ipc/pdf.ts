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

/**
 * SPEC: P1-VIEW-003 — the optional `password` is sent on retry after a
 * `PasswordRequired` error from a previous attempt. Callers that don't
 * care about encryption pass nothing; PDFium treats an absent password
 * as "no password supplied" and rejects encrypted documents with the
 * same code as a wrong password, which the dialog flow handles.
 */
export async function openPdfPath(
  path: string,
  password?: string,
): Promise<OpenedDocument> {
  return invoke<OpenedDocument>("pdf_open", { path, password: password ?? null });
}

export async function closePdf(id: DocumentId): Promise<void> {
  return invoke<void>("pdf_close", { id });
}

/**
 * Wire-format selector for {@link renderPage}.
 *
 * - `"png"` — PNG bytes, decode via `<img src="data:...">` or `Blob`.
 * - `"rgba8"` — raw 8-bit RGBA pixels, drop straight onto a canvas
 *   via `new ImageData(...)`.
 *
 * Mirrors `pdf::render::ImageFormat` in Rust.
 */
export type ImageFormat = "png" | "rgba8";

/**
 * Reply payload of {@link renderPage}.
 *
 * `bytes` arrives as a plain `number[]`: the Rust side returns
 * `Vec<u8>`, which serde serializes as a JSON array of numbers, so
 * Tauri's `invoke` hands back a `number[]` (NOT a `Uint8Array` — that
 * was an earlier mislabel). Wrap it with `Uint8Array.from(bytes)` before
 * use. (The raw-bytes upgrade path — `tauri::ipc::Response` — is noted
 * in `pdf::render::RenderedPage`.)
 *
 * Mirrors `pdf::render::RenderedPage` in Rust.
 */
export interface RenderedPage {
  width: number;
  height: number;
  format: ImageFormat;
  bytes: number[];
}

/** SPEC: P1-VIEW-008 + NFR-PERF-003. Routed through the actor. */
export async function renderPage(
  id: DocumentId,
  page: number,
  dpi: number,
  format: ImageFormat,
): Promise<RenderedPage> {
  return invoke<RenderedPage>("pdf_render_page", { id, page, dpi, format });
}

export async function pdfiumVersion(): Promise<string> {
  return invoke<string>("pdfium_version");
}

/**
 * Edit-preview pipeline: the actor's *live* in-memory document serialized
 * to bytes. The Rust side returns raw bytes via `tauri::ipc::Response`, so
 * `invoke` resolves to an `ArrayBuffer` (not a JSON `number[]`) — essential
 * for large documents, where the old array-of-numbers encoding ballooned a
 * 13 MB file to ~50 MB of JSON per edit and stalled the reload (P4.HF28).
 * Wrapped as a `Uint8Array` for PDF.js `getDocument({ data })`. Reflects
 * unsaved edits (rotate, ink, text box, …) so the view updates without a reopen.
 *
 * `new Uint8Array(buf)` accepts an `ArrayBuffer` (the Response path), and also
 * a `number[]` or typed array, so it stays correct if the transport ever changes.
 */
export async function getPdfBytes(id: DocumentId): Promise<Uint8Array> {
  const buf = await invoke<ArrayBuffer>("pdf_get_bytes", { id });
  return new Uint8Array(buf);
}

/**
 * SPEC: P6-SEC-007 (P6.C1) — write a password-protected copy of `id` to `path`,
 * with AES-256 encryption.
 *
 * **Protect-on-export.** The open document is untouched: this produces an
 * encrypted copy and leaves the current file, its undo history and its password
 * alone. Encrypting in place would silently change the password the open
 * document needs, which is a good way to lock someone out of their own work.
 *
 * Both passwords are optional and mean different things — user gates *opening*,
 * owner gates *permissions* — but at least one is required; the backend rejects
 * a request with neither. The passwords go straight to the Rust side and are
 * never stored, logged, or echoed back.
 */
export async function protectPdf(
  id: DocumentId,
  path: string,
  userPassword: string | null,
  ownerPassword: string | null,
): Promise<void> {
  return invoke<void>("pdf_protect", { id, path, userPassword, ownerPassword });
}
