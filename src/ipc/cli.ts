// SPEC: P1-VIEW-001 — CLI-arg open. The Rust side buffers PDF paths
// parsed from argv at startup; the frontend drains them once on mount
// via this wrapper. See `src-tauri/src/commands/cli.rs` for why this
// is a pull command rather than a setup-time event.

import { invoke } from "@/ipc/invoke";

/** Drain the CLI-pending buffer. Returns the paths once; subsequent calls return []. */
export async function takePendingCliOpens(): Promise<string[]> {
  return invoke<string[]>("cli_take_pending_opens");
}
