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
}

impl AppState {
    fn new() -> Self {
        Self {
            actors: Mutex::new(HashMap::new()),
            recents_lock: Mutex::new(()),
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
            app.manage(AppState::new());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::pdf::pdf_open,
            commands::pdf::pdf_close,
            commands::pdf::pdf_render_page,
            commands::pdf::pdfium_version,
            commands::recents::recents_list,
            commands::recents::recents_push,
            commands::recents::recents_clear,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
