// SPEC: P1-VIEW-011 — typed wrappers around the session-restore IPC.
// The Rust side owns session.json; this mirrors the Rust `Session`
// struct. Only paths are stored — per-doc view state lives in C2's
// IndexedDB and reattaches when the document re-opens.

import { invoke } from "@/ipc/invoke";

export interface SessionState {
  /** Open document paths, in tab order. */
  open: string[];
  /** Path of the active tab, or null. Guaranteed by the backend to be a member of `open` (or null). */
  active: string | null;
}

/** Read the persisted session (open tabs + active). */
export async function loadSession(): Promise<SessionState> {
  return invoke<SessionState>("session_load");
}

/** Persist the current open tabs + active tab. */
export async function saveSession(
  open: string[],
  active: string | null,
): Promise<void> {
  return invoke<void>("session_save", { open, active });
}
