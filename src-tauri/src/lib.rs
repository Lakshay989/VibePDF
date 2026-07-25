#![warn(clippy::unwrap_used, clippy::expect_used)]
#![warn(clippy::pedantic)]
#![allow(
    clippy::module_name_repetitions,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]

pub mod commands;
pub mod error;
pub mod pdf;
pub mod settings;

use std::collections::HashMap;
use std::sync::Mutex;

use tauri::Manager;
use tracing_subscriber::EnvFilter;

use crate::pdf::actor::DocumentActorHandle;

/// Process-wide state held by Tauri. Each open document is reachable
/// through its actor; the dispatcher routes IPC messages by id.
pub struct AppState {
    pub actors: Mutex<HashMap<uuid::Uuid, DocumentActorHandle>>,
    /// SPEC: P1-VIEW-012 — guards the read-modify-write of
    /// `recents.json` so two quick opens can't race and drop an entry.
    /// The mutex protects the *file*, not in-memory data, so it holds
    /// `()`.
    pub recents_lock: Mutex<()>,
    /// SPEC: P1-VIEW-011 — guards `session.json` writes the same way.
    pub session_lock: Mutex<()>,
    /// SPEC: P1-VIEW-001 (CLI-arg clause) — PDF paths parsed from
    /// `argv` at startup, drained once by the frontend on mount via
    /// `cli_take_pending_opens`. See `commands/cli.rs` for the
    /// emit-before-listener race this sidesteps.
    pub cli_pending: Mutex<Vec<String>>,
}

impl AppState {
    fn new() -> Self {
        Self {
            actors: Mutex::new(HashMap::new()),
            recents_lock: Mutex::new(()),
            session_lock: Mutex::new(()),
            cli_pending: Mutex::new(Vec::new()),
        }
    }
}

/// Entry point invoked from `main.rs`. Lives in `lib` so we can also
/// instantiate the runtime from integration tests if we want to.
///
/// `expect()` is allowed here: this function is the de-facto `main` —
/// any failure to start the Tauri runtime is fatal and the user will
/// see the panic message in `stderr`. Convention (`docs/06_CONVENTIONS.md`)
/// permits `expect` in `main.rs`-equivalent code paths.
#[allow(clippy::expect_used)]
pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with_target(false)
        .json()
        .try_init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            let state = AppState::new();
            // SPEC: P1-VIEW-001 (CLI-arg clause) — parse argv into the
            // pending-opens buffer here, while we still have the
            // process startup context. The frontend drains it on
            // mount; see commands/cli.rs for the design rationale.
            let cli_paths: Vec<String> =
                commands::cli::pdf_paths_from_args(std::env::args())
                    .into_iter()
                    .filter(|p| std::path::Path::new(p).is_file())
                    .collect();
            if !cli_paths.is_empty() {
                tracing::info!(count = cli_paths.len(), "buffered CLI-pending opens");
                if let Ok(mut guard) = state.cli_pending.lock() {
                    *guard = cli_paths;
                }
            }
            app.manage(state);
            // SPEC: P2.A2 — start the 30s autosave tick. Pokes each open
            // actor to write a recovery copy if dirty. Dedicated std
            // thread (no tokio "time" feature); runs for the app's life.
            pdf::autosave::spawn_autosave_tick(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::pdf::pdf_open,
            commands::pdf::pdf_close,
            commands::pdf::pdf_render_page,
            commands::pdf::pdfium_version,
            commands::pdf::pdf_rotate_pages,
            commands::pdf::pdf_resize_pages,
            commands::pdf::pdf_add_text_markup,
            commands::pdf::pdf_clear_text_markup,
            commands::pdf::pdf_add_text_note,
            commands::pdf::pdf_update_text_note,
            commands::pdf::pdf_delete_annotation,
            commands::pdf::pdf_read_text_notes,
            commands::pdf::pdf_read_annotations,
            commands::pdf::pdf_export_annotations,
            commands::pdf::pdf_import_annotations,
            commands::pdf::pdf_flatten_annotations,
            commands::pdf::pdf_add_free_text,
            commands::pdf::pdf_add_text_box,
            commands::pdf::pdf_update_text_box,
            commands::pdf::pdf_read_text_boxes,
            commands::pdf::pdf_add_image,
            commands::pdf::pdf_add_link,
            commands::pdf::pdf_add_text_watermark,
            commands::pdf::pdf_add_image_watermark,
            commands::pdf::pdf_add_color_background,
            commands::pdf::pdf_add_image_background,
            commands::pdf::pdf_add_pdf_background,
            commands::pdf::pdf_add_header_footer,
            commands::pdf::pdf_extract_images,
            commands::pdf::pdf_transform_image,
            commands::pdf::pdf_delete_image,
            commands::pdf::pdf_replace_image,
            commands::pdf::pdf_read_free_text,
            commands::pdf::pdf_update_free_text,
            commands::pdf::pdf_add_shape,
            commands::pdf::pdf_add_line,
            commands::pdf::pdf_add_polygon,
            commands::pdf::pdf_add_ink,
            commands::pdf::pdf_add_stamp,
            commands::pdf::pdf_add_image_stamp,
            commands::pdf::pdf_add_measure,
            commands::pdf::pdf_read_measure_calibration,
            commands::pdf::pdf_extract_text_runs,
            commands::pdf::pdf_read_font_report,
            commands::pdf::pdf_replace_text_run,
            commands::pdf::pdf_delete_text_run,
            commands::pdf::pdf_add_reply,
            commands::pdf::pdf_delete_pages,
            commands::pdf::pdf_insert_blank_page,
            commands::pdf::pdf_crop_page,
            commands::pdf::pdf_extract_pages,
            commands::pdf::pdf_split_document,
            commands::pdf::pdf_merge_documents,
            commands::pdf::pdf_insert_from_pdf,
            commands::pdf::pdf_reorder_pages,
            commands::pdf::pdf_peek_page_count,
            commands::pdf::pdf_get_bytes,
            commands::save::pdf_save,
            commands::history::pdf_undo,
            commands::history::pdf_redo,
            commands::history::pdf_history_state,
            commands::recovery::recovery_list,
            commands::recovery::recovery_discard,
            commands::recents::recents_list,
            commands::recents::recents_push,
            commands::recents::recents_clear,
            commands::session::session_load,
            commands::session::session_save,
            commands::cli::cli_take_pending_opens,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
