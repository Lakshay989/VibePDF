//! SPEC: P1-VIEW-012 — recents list invariants and disk round-trip.
//!
//! These exercise `settings::recents` directly (the `AppHandle`-free
//! layer), so no Tauri app is needed. Temp files live under the OS temp
//! dir with a uuid suffix — we reuse the `uuid` dep rather than pull in
//! `tempfile`, and clean up on the happy path.

use std::path::PathBuf;

use vibepdf_lib::settings::recents::{self, MAX_RECENTS};

/// Unique scratch file path under the OS temp dir.
fn scratch() -> PathBuf {
    std::env::temp_dir().join(format!("vibepdf-recents-test-{}.json", uuid::Uuid::new_v4()))
}

#[test]
fn push_dedups_and_moves_to_front() {
    let mut list = vec!["/a.pdf".to_string(), "/b.pdf".to_string()];
    recents::push_front(&mut list, "/b.pdf".to_string());
    // /b moves to front, no duplicate.
    assert_eq!(list, vec!["/b.pdf".to_string(), "/a.pdf".to_string()]);
}

#[test]
fn push_caps_at_twenty_pruning_oldest() {
    let mut list = Vec::new();
    // Push 25 distinct paths; only the last 20 should survive, newest first.
    for i in 0..25 {
        recents::push_front(&mut list, format!("/f{i}.pdf"));
    }
    assert_eq!(list.len(), MAX_RECENTS);
    // Most-recent push (/f24) is at the front; the 5 oldest (/f0../f4) are gone.
    assert_eq!(list.first().map(String::as_str), Some("/f24.pdf"));
    assert_eq!(list.last().map(String::as_str), Some("/f5.pdf"));
    assert!(!list.iter().any(|p| p == "/f4.pdf"));
}

#[test]
fn load_round_trips_through_disk() {
    let file = scratch();
    let list = vec!["/x.pdf".to_string(), "/y.pdf".to_string()];
    recents::save(&file, &list).expect("save should succeed");
    let loaded = recents::load(&file);
    assert_eq!(loaded, list);
    let _ = std::fs::remove_file(&file);
}

#[test]
fn load_missing_file_returns_empty() {
    // A path that was never written — first-run behaviour.
    let file = scratch();
    assert!(recents::load(&file).is_empty());
}

#[test]
fn load_corrupt_file_returns_empty() {
    // Defensive: a hand-mangled / truncated file must not error.
    let file = scratch();
    std::fs::write(&file, b"{not valid json").expect("write garbage");
    assert!(recents::load(&file).is_empty());
    let _ = std::fs::remove_file(&file);
}

#[test]
fn save_then_clear_empties_disk() {
    let file = scratch();
    recents::save(&file, &["/a.pdf".to_string()]).expect("save");
    recents::save(&file, &[]).expect("clear via empty save");
    assert!(recents::load(&file).is_empty());
    let _ = std::fs::remove_file(&file);
}
