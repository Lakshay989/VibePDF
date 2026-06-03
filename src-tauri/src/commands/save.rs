use std::path::PathBuf;

use tauri::State;

use crate::error::CommandError;
use crate::pdf::document::SaveOutcome;
use crate::AppState;

/// SPEC: P2-SAVE-001 — explicit save (Cmd/Ctrl+S) and save-as.
///
/// `path = None` writes the document back to its own path (a no-op when
/// there are no unsaved changes); `Some(p)` is a save-as to `p`. The
/// write itself — atomic temp+rename, `.bak` rotation, and round-trip
/// verification — runs on the document actor thread (`PDFium` is not
/// thread-safe per document); see `pdf::document::save_document`.
#[tauri::command]
pub async fn pdf_save(
    state: State<'_, AppState>,
    id: String,
    path: Option<String>,
) -> Result<SaveOutcome, CommandError> {
    let uuid = uuid::Uuid::parse_str(&id)
        .map_err(|_| CommandError::InvalidInput(format!("not a UUID: {id}")))?;

    // Send the request while holding the map lock, then drop the lock
    // before awaiting — otherwise the actor map is held across an
    // `.await`, blocking every other command on every other document for
    // the duration of the save (mirrors `pdf_render_page`).
    let rx = {
        let guard = state
            .actors
            .lock()
            .map_err(|e| CommandError::Internal(format!("actor map poisoned: {e}")))?;
        let handle = guard
            .get(&uuid)
            .ok_or_else(|| CommandError::NotFound(format!("document {id}")))?;
        handle.save_request(path.map(PathBuf::from))?
    };

    rx.await
        .map_err(|_| CommandError::Internal("doc-actor dropped reply".into()))?
}
