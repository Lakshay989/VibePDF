import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@/ipc/invoke";
import type { HistoryState } from "@/ipc/history";

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
/**
 * SPEC: P6-SEC-009 (P6.C3) — what a reader may do with the protected copy.
 *
 * Every field means *allowed*, so `true` everywhere is an unrestricted
 * document. Note that these are advisory: no PDF enforces them, readers may
 * ignore them, and a document whose permissions password equals its open
 * password can have them lifted by anyone who can open it.
 */
export interface DocumentPermissions {
  print: boolean;
  copy: boolean;
  modify: boolean;
  fillForms: boolean;
  annotate: boolean;
  extract: boolean;
  assemble: boolean;
}

/** Everything allowed — the shape a caller that sets nothing should send. */
export const ALL_PERMISSIONS: DocumentPermissions = {
  print: true,
  copy: true,
  modify: true,
  fillForms: true,
  annotate: true,
  extract: true,
  assemble: true,
};

export async function protectPdf(
  id: DocumentId,
  path: string,
  userPassword: string | null,
  ownerPassword: string | null,
  permissions: DocumentPermissions = ALL_PERMISSIONS,
): Promise<void> {
  return invoke<void>("pdf_protect", {
    id,
    path,
    userPassword,
    ownerPassword,
    permissions,
  });
}

/**
 * SPEC: P6-SEC-008 (P6.C2) — write an unprotected copy of `id` to `path`.
 *
 * `password` is the document's **owner** password. For AES-256 that is what
 * lopdf enforces; the user password is refused. See `security/decrypt.rs`.
 *
 * The open document is untouched — this is an export, mirroring `protectPdf`.
 * The backend re-opens the output with no password before returning, so a file
 * that is still encrypted never reaches the user.
 */
export async function removePdfProtection(
  id: DocumentId,
  path: string,
  password: string,
): Promise<void> {
  return invoke<void>("pdf_remove_protection", { id, path, password });
}

/**
 * SPEC: P6-SEC-012 (P6.D3) — what "Clean document" should remove.
 *
 * Every field is a *removal*, so all-false is the document untouched. That is
 * the opposite sense to `DocumentPermissions`, where a field means "allowed" —
 * worth keeping straight, because both are seven booleans on a dialog.
 */
export interface CleanOptions {
  /** `/Info`, the XMP packet, and any page-level metadata. */
  metadata: boolean;
  /** Text drawn invisibly — including the OCR layer of a scanned page. */
  hiddenText: boolean;
  /** Markup annotations. Not links, form fields, or attachments. */
  comments: boolean;
  attachments: boolean;
  bookmarks: boolean;
  /** Field values; the empty form stays. */
  formData: boolean;
  embeddedFiles: boolean;
}

/** Nothing removed — the starting state of the dialog. */
export const CLEAN_NOTHING: CleanOptions = {
  metadata: false,
  hiddenText: false,
  comments: false,
  attachments: false,
  bookmarks: false,
  formData: false,
  embeddedFiles: false,
};

/**
 * What a clean removed. The visible page is unchanged by design, so these
 * counts are the only evidence the command did anything.
 */
export interface CleanReport {
  infoKeys: number;
  xmpPackets: number;
  hiddenTextRuns: number;
  comments: number;
  attachments: number;
  bookmarks: number;
  formFields: number;
  embeddedFiles: number;
  history: HistoryState;
}

/**
 * SPEC: P6-SEC-012 (P6.D3) — clean the open document in place. Undoable
 * in-session; permanent once the file is saved and reopened.
 *
 * In place rather than on export, unlike protect/unlock: you want to watch the
 * comments go, and you want Undo if you cleaned more than you meant to.
 */
export async function cleanDocument(
  id: DocumentId,
  options: CleanOptions,
): Promise<CleanReport> {
  return invoke<CleanReport>("pdf_clean_document", { id, options });
}

/**
 * SPEC: P6-SEC-005 (P6.B1b) — how much a reader may change a *certified*
 * document without invalidating the signature (PDF 32000-1 §12.8.2.2, DocMDP).
 *
 * Advisory, like the encryption permissions in P6.C3: nothing enforces it. What
 * it buys is detection, not prevention — a conforming reader that makes a
 * disallowed change afterwards reports the signature as invalid, so the change
 * shows. "Lock" sounds like prevention and is not.
 */
export type DocMdpLevel = "noChanges" | "formFilling" | "formFillingAndAnnotations";

/** What goes in the signature dictionary alongside the signature itself. */
export interface SignatureDetails {
  /** PDF date string — use `pdfDate(new Date())`. */
  signedAt: string;
  reason: string | null;
  location: string | null;
  /** Display name. Not a security claim; the certificate is. */
  name: string | null;
  /**
   * Non-null makes this a **certification** signature — a claim about the whole
   * document, and only the first signature on one may make it. `null` is an
   * ordinary approval signature, which is the common case.
   */
  certify: DocMdpLevel | null;
}

/**
 * SPEC: P6-SEC-005 (P6.B1a) — write a certificate-signed copy of `id` to `path`.
 *
 * **Sign-on-export, and necessarily so.** Saving a document re-serialises it,
 * which rewrites every byte offset — and a signature covers exact bytes. Signing
 * in place would give you a file that the next Save silently un-signs, still
 * looking signed until someone checked. So the signed copy is written to disk
 * and the open document is left alone.
 *
 * `certificatePath` is a path, not bytes: the private key has no reason to cross
 * the IPC boundary. Neither it nor `password` is logged anywhere.
 */
export async function signPdf(
  id: DocumentId,
  path: string,
  certificatePath: string,
  password: string,
  details: SignatureDetails,
): Promise<void> {
  return invoke<void>("pdf_sign_document", {
    id,
    path,
    certificate: certificatePath,
    password,
    details,
  });
}

/**
 * SPEC: P6-SEC-006 (P6.B2b) — what VibePDF can say about a certificate's issuer.
 *
 * There is deliberately no `trusted`. We have no trust anchors and none we can
 * bundle, so the claim is not representable rather than merely undocumented.
 * See `security/verify.rs`.
 */
export type ChainStatus = "selfSigned" | "issuerNotChecked" | "incomplete" | "broken";

/** SPEC: P6-SEC-006 — the per-signature status. */
export interface SignatureReport {
  fieldName: string | null;
  /** The signing certificate's subject. */
  signer: string;
  issuer: string;
  /** `/M` — the *claimed* signing time. Nothing here proves it. */
  signedAt: string | null;
  reason: string | null;
  /** The signature over the signed attributes checks out. */
  signatureValid: boolean;
  /** The document still hashes to what was signed. */
  digestMatches: boolean;
  /** Nothing was appended after this signature. */
  coversWholeDocument: boolean;
  certificateExpired: boolean;
  chain: ChainStatus;
  /** DocMDP `/P`, when this signature certifies the document. */
  certificationLevel: number | null;
  /** Anything that stopped a check from running. */
  problems: string[];
}

/**
 * SPEC: P6-SEC-006 — verify every signature on the open document.
 *
 * Reports on the file **as saved**: a signature covers exact byte offsets, and
 * the in-memory document would have to be re-serialised to inspect, which moves
 * every one of them. Unsaved edits are therefore not reflected — and saving
 * would invalidate the signature anyway.
 */
export async function verifySignatures(id: DocumentId): Promise<SignatureReport[]> {
  return invoke<SignatureReport[]>("pdf_verify_signatures", { id });
}
