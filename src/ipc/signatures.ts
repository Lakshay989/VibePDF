// P6.A1 — typed wrappers for the local signature library.
//
// Infrastructure for P6-SEC-001/-002/-003: the store a drawn, typed, or
// imported signature is saved into. The library lives under `app_data_dir`;
// only the Rust command layer knows where, so nothing here takes a path.
//
// Bytes cross the IPC boundary as a number array (Tauri's `Vec<u8>`), which
// costs roughly 4× the raw size in JSON. Fine for a ~10–50 KB signature; it
// would not be for anything larger.

import { invoke } from "@/ipc/invoke";

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
