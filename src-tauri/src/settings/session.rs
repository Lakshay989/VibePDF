//! SPEC: P1-VIEW-011 — session restore. Persists which document tabs
//! were open (by path) and which was active, to
//! `<app_data_dir>/session.json`, so a relaunch can re-open them.
//!
//! Only *paths* are stored here. Per-document view state (zoom, scroll,
//! page) lives in C2's per-document `IndexedDB` and reattaches when the
//! document re-opens. This module is `AppHandle`-free and unit-tested
//! directly; the command layer (`commands/session.rs`) resolves the
//! path and takes the lock.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::CommandError;
use crate::settings::{read_json, write_atomic};

const CURRENT_VERSION: u32 = 1;

/// The restorable session: the open tabs (in order) and which one was
/// active. `active` is a path that should appear in `open`; `load`
/// coerces it to `None` if it doesn't (e.g. the active tab's file was
/// removed and dropped from `open`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub open: Vec<String>,
    pub active: Option<String>,
}

/// On-disk shape. `version` is carried for forward-compat.
#[derive(Debug, Default, Serialize, Deserialize)]
struct SessionFile {
    version: u32,
    open: Vec<String>,
    active: Option<String>,
}

/// Read the session from `file`. Missing/corrupt/wrong-version → an
/// empty session (first run / defensive). A stored `active` that is no
/// longer present in `open` is coerced to `None`.
#[must_use]
pub fn load(file: &Path) -> Session {
    match read_json::<SessionFile>(file) {
        Some(parsed) if parsed.version == CURRENT_VERSION => {
            let active = parsed
                .active
                .filter(|a| parsed.open.iter().any(|p| p == a));
            Session {
                open: parsed.open,
                active,
            }
        }
        _ => Session::default(),
    }
}

/// Atomically persist `session` to `file`. See `settings::write_atomic`.
pub fn save(file: &Path, session: &Session) -> Result<(), CommandError> {
    let payload = SessionFile {
        version: CURRENT_VERSION,
        open: session.open.clone(),
        active: session.active.clone(),
    };
    let json = serde_json::to_vec_pretty(&payload)
        .map_err(|e| CommandError::Internal(format!("serialize session: {e}")))?;
    write_atomic(file, &json)
}
