//! Persisted application settings owned by the Rust side.
//!
//! Per `docs/04_ARCHITECTURE.md` § "Settings storage", app-wide
//! settings live under `app_config_dir` / `app_data_dir` and the Rust
//! side is their source of truth; the frontend mirrors a derived view.

pub mod recents;
