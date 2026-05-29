//! SPEC: P1-VIEW-001 — the command-line-argument clause.
//!
//! `setup` parses `std::env::args()` once (see `lib.rs`), keeps the
//! `.pdf` paths that exist on disk, and buffers them in
//! `AppState.cli_pending`. The frontend drains the buffer on mount via
//! `cli_take_pending_opens` and routes each path through the normal
//! `openByPath` — so CLI files inherit recents (A3), session
//! persistence (E1), and the password prompt (B2) for free.
//!
//! ### Why a pull command, not a `cli-open` event
//!
//! `steps/P1.md` floats emitting a `cli-open` event in `setup`, but
//! `setup` runs before the webview / React mount, so the listener does
//! not exist yet and the event is silently dropped. A drain command
//! sidesteps the race entirely.

use tauri::State;

use crate::error::CommandError;
use crate::AppState;

/// Filter `args` to PDF paths. Pure: no IO, just extension matching.
///
/// - Drops `argv[0]` (the binary path), matching standard CLI conventions.
/// - Keeps anything ending in `.pdf` (case-insensitive).
/// - Preserves input order.
///
/// Existence checking happens at the call site in `lib.rs::setup`, so
/// the pure logic stays trivially unit-testable in `tests/cli_open.rs`.
pub fn pdf_paths_from_args<I, S>(args: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    args.into_iter()
        .skip(1)
        .map(Into::into)
        .filter(|s| {
            // ends_with on a lowercased copy avoids allocating on every
            // non-pdf arg (.to_lowercase only fires when we have to).
            s.len() >= 4 && s[s.len() - 4..].eq_ignore_ascii_case(".pdf")
        })
        .collect()
}

/// SPEC: P1-VIEW-001 — drain the CLI-pending buffer. Returns the
/// paths once and clears them so a refresh / remount can't re-open
/// the same files.
#[tauri::command]
pub async fn cli_take_pending_opens(
    state: State<'_, AppState>,
) -> Result<Vec<String>, CommandError> {
    let mut guard = state
        .cli_pending
        .lock()
        .map_err(|e| CommandError::Internal(format!("cli_pending lock poisoned: {e}")))?;
    Ok(std::mem::take(&mut *guard))
}
