//! Persisted application settings owned by the Rust side.
//!
//! Per `docs/04_ARCHITECTURE.md` § "Settings storage", app-wide
//! settings live under `app_config_dir` / `app_data_dir` and the Rust
//! side is their source of truth; the frontend mirrors a derived view.
//!
//! Both persisted files (`recents.json`, `session.json`) share the same
//! durability + defensive-read posture, lifted here so each concrete
//! module (`recents`, `session`) is just its data shape plus a thin
//! load/save:
//!
//! - **`read_json`** — returns `None` on a missing *or* corrupt file.
//!   Persisted settings are conveniences; a mangled file must never
//!   block the app, so callers substitute a default.
//! - **`write_atomic`** — writes to a uuid-suffixed sibling then
//!   renames over the target. `rename` within a filesystem is atomic on
//!   every platform we target, so a crash mid-write leaves the previous
//!   file intact rather than a truncated one. Creates the parent dir on
//!   first run (before Tauri has materialised `app_data_dir`).

use std::path::Path;

use serde::de::DeserializeOwned;

use crate::error::CommandError;

pub mod recents;
pub mod session;
pub mod signatures;

/// Deserialize JSON of type `T` from `file`. `None` when the file is
/// absent or fails to parse — callers fall back to a default. Never
/// errors: a corrupt settings file is not a reason to fail a command.
#[must_use]
pub fn read_json<T: DeserializeOwned>(file: &Path) -> Option<T> {
    let bytes = std::fs::read(file).ok()?;
    serde_json::from_slice::<T>(&bytes).ok()
}

/// Atomically write `bytes` to `file` via a temp sibling + rename.
/// Creates the parent directory if missing.
pub fn write_atomic(file: &Path, bytes: &[u8]) -> Result<(), CommandError> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Unique temp sibling so two concurrent writers can't clobber each
    // other's temp before the rename. Callers already hold a per-file
    // mutex, but the unique suffix is cheap insurance.
    let tmp = file.with_extension(format!("tmp-{}", uuid::Uuid::new_v4()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, file)?;
    Ok(())
}
