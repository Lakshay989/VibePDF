// P6.A1 — typed wrappers for the local signature library.
//
// Infrastructure for P6-SEC-001/-002/-003: the store a drawn, typed, or
// imported signature is saved into. The library lives under `app_data_dir`;
// only the Rust command layer knows where, so nothing here takes a path.
//
// Bytes cross the IPC boundary as a number array (Tauri's `Vec<u8>`), which
// costs roughly 4× the raw size in JSON. Fine for a ~10–50 KB signature; it
// would not be for anything larger.

import type { HistoryState } from "@/ipc/history";
import { invoke } from "@/ipc/invoke";
import type { DocumentId } from "@/ipc/pdf";

/** How a signature was produced. The stored bytes are always PNG. */
export type SignatureKind = "draw" | "type" | "image";

/** One library entry. The bytes are fetched separately by `signatureBytes`. */
export interface SignatureEntry {
  /** Opaque id — also the blob's filename stem on disk. */
  id: string;
  kind: SignatureKind;
  /** Unix milliseconds. The list is ordered newest-first by this. */
  createdAt: number;
}

/** P6.A1 — the stored signatures, newest first. Empty on a fresh install. */
export async function listSignatures(): Promise<SignatureEntry[]> {
  return invoke<SignatureEntry[]>("signatures_list");
}

/**
 * P6.A1 — store `png` as a new signature. Rejects non-PNG bytes; the oldest
 * entries are pruned once the library is full.
 */
export async function addSignature(
  kind: SignatureKind,
  png: Uint8Array,
): Promise<SignatureEntry> {
  return invoke<SignatureEntry>("signatures_add", { kind, bytes: Array.from(png) });
}

/** P6.A1 — delete a signature and its blob. An unknown id is a no-op. */
export async function removeSignature(id: string): Promise<void> {
  return invoke<void>("signatures_remove", { id });
}

/** P6.A1 — the PNG bytes for `id`, for a preview or for placing it. */
export async function signatureBytes(id: string): Promise<Uint8Array> {
  const bytes = await invoke<number[]>("signatures_bytes", { id });
  return Uint8Array.from(bytes);
}

/**
 * SPEC: P6-SEC-004 (P6.A5a) — place signature `signatureId` on `page` (0-based)
 * of `documentId`, aspect-correct around `(x, y)` in PDF points at `height`
 * points tall.
 *
 * The **id** crosses the boundary, not the bytes: the backend resolves it
 * against the library, so ~30KB of PNG never makes the round trip as JSON. It
 * lands as a `/Stamp` with the alpha channel as an `/SMask`, and is undoable.
 *
 * This is the stamp half of P6-SEC-004 only. Writing into an existing `/Sig`
 * field as a PKCS#7 signature needs P6.B1; callers must decline that case
 * rather than stamping over the field — see `tools/signature/place.ts`.
 */
export async function placeSignature(
  documentId: DocumentId,
  page: number,
  x: number,
  y: number,
  height: number,
  signatureId: string,
  opacity: number,
): Promise<HistoryState> {
  return invoke<HistoryState>("pdf_place_signature", {
    id: documentId,
    page,
    x,
    y,
    height,
    signatureId,
    opacity,
  });
}
