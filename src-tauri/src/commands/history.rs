use tauri::State;

use crate::error::CommandError;
use crate::pdf::undo::HistoryState;
use crate::AppState;

/// Resolve `id` to an actor and run `send` (a send-only request method on
/// the handle) while holding the map lock, returning the reply receiver.
/// The lock is dropped before the caller awaits — same discipline as
/// `pdf_render_page` / `pdf_save`.
macro_rules! request {
    ($state:expr, $id:expr, $method:ident) => {{
        let uuid = uuid::Uuid::parse_str(&$id)
            .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {}", $id)))?;
        let guard = $state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {}", $id)))?;
        handle.$method()?
    }};
}

/// SPEC: P2-PAGE-003 / session history — undo the most recent edit.
/// A no-op (returns the unchanged availability) when nothing is undoable.
#[tauri::command]
pub async fn pdf_undo(
    state: State<'_, AppState>,
    id: String,
) -> Result<HistoryState, CommandError> {
    let rx = request!(state, id, undo_request);
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// Redo the most recently undone edit. No-op when nothing is redoable.
#[tauri::command]
pub async fn pdf_redo(
    state: State<'_, AppState>,
    id: String,
) -> Result<HistoryState, CommandError> {
    let rx = request!(state, id, redo_request);
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}

/// Current undo/redo availability, for hydrating the UI button state.
#[tauri::command]
pub async fn pdf_history_state(
    state: State<'_, AppState>,
    id: String,
) -> Result<HistoryState, CommandError> {
    let rx = request!(state, id, history_state_request);
    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))
}
