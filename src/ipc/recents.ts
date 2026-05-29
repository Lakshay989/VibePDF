// SPEC: P1-VIEW-012 — typed wrappers around the recents IPC commands.
// The Rust side owns the list (cap-at-20, dedup, persistence to
// app_data_dir/recents.json); every wrapper returns the post-mutation
// list so the frontend store can mirror it without re-deriving order.

import { invoke } from "@/ipc/invoke";

/** Read the persisted recents (most-recent first). */
export async function listRecents(): Promise<string[]> {
  return invoke<string[]>("recents_list");
}

/** Record `path` as most-recent; returns the new list. */
export async function pushRecent(path: string): Promise<string[]> {
  return invoke<string[]>("recents_push", { path });
}

/** Clear the list on UI and disk; returns the empty list. */
export async function clearRecents(): Promise<string[]> {
  return invoke<string[]>("recents_clear");
}
