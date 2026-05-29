//! SPEC: P1-VIEW-011 — session persistence round-trip and defensive
//! load. Exercises `settings::session` directly (the `AppHandle`-free
//! layer); no Tauri app needed. Temp files use the `uuid` dep, cleaned
//! up on the happy path.

use std::path::PathBuf;

use vibepdf_lib::settings::session::{self, Session};

fn scratch() -> PathBuf {
    std::env::temp_dir().join(format!("vibepdf-session-test-{}.json", uuid::Uuid::new_v4()))
}

#[test]
fn session_round_trips_open_and_active() {
    let file = scratch();
    let s = Session {
        open: vec!["/a.pdf".to_string(), "/b.pdf".to_string()],
        active: Some("/b.pdf".to_string()),
    };
    session::save(&file, &s).expect("save");
    assert_eq!(session::load(&file), s);
    let _ = std::fs::remove_file(&file);
}

#[test]
fn session_missing_file_is_empty() {
    // First-run behaviour: nothing persisted yet.
    let file = scratch();
    assert_eq!(session::load(&file), Session::default());
}

#[test]
fn session_corrupt_file_is_empty() {
    let file = scratch();
    std::fs::write(&file, b"{ truncated").expect("write garbage");
    assert_eq!(session::load(&file), Session::default());
    let _ = std::fs::remove_file(&file);
}

#[test]
fn active_not_in_open_coerces_to_none() {
    // SPEC: P1-VIEW-011 — if the active tab's file dropped out of the
    // open set (e.g. it was deleted), `active` must not dangle.
    let file = scratch();
    let s = Session {
        open: vec!["/a.pdf".to_string()],
        active: Some("/gone.pdf".to_string()),
    };
    session::save(&file, &s).expect("save");
    let loaded = session::load(&file);
    assert_eq!(loaded.open, vec!["/a.pdf".to_string()]);
    assert_eq!(loaded.active, None);
    let _ = std::fs::remove_file(&file);
}

#[test]
fn empty_session_round_trips() {
    // Quitting with no docs open must persist cleanly and reload empty.
    let file = scratch();
    session::save(&file, &Session::default()).expect("save");
    assert_eq!(session::load(&file), Session::default());
    let _ = std::fs::remove_file(&file);
}
