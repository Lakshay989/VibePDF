import { invoke } from "@/ipc/invoke";

/**
 * A document with an unsaved-changes recovery copy on disk. Mirrors
 * `pdf::autosave::RecoveryEntry` on the Rust side.
 *
 * `autosavePath` is the recovery copy under `app_data_dir`; `originalPath`
 * is where the user's file lives. `savedAt` is unix seconds.
 */
export interface RecoveryEntry {
  documentId: string;
  originalPath: string;
  autosavePath: string;
  savedAt: number;
}

/** SPEC: P2.A2 — recoverable documents found at startup. */
export async function recoveryList(): Promise<RecoveryEntry[]> {
  return invoke<RecoveryEntry[]>("recovery_list");
}

/** SPEC: P2.A2 — drop a recovery copy once recovered or declined. */
export async function recoveryDiscard(id: string): Promise<void> {
  return invoke<void>("recovery_discard", { id });
}
