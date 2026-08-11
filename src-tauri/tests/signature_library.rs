//! Integration tests for the signature library (P6.A1).
//!
//! Infrastructure for P6-SEC-001/-002/-003 — "the local signature library" the
//! spec names as where a created signature is saved. Everything runs against a
//! temp directory; nothing here touches `app_data_dir`.

use std::path::PathBuf;

use vibepdf_lib::error::CommandError;
use vibepdf_lib::settings::signatures::{
    self, blob_path, index_path, SignatureKind, MAX_SIGNATURES,
};

/// A fresh temp directory, removed when the guard drops.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!("vibepdf-sig-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).expect("mkdir");
        Self(p)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The smallest valid PNG: signature + IHDR + IDAT + IEND. Only the 8-byte
/// magic is actually checked, but a real one keeps the fixture honest.
fn png(seed: u8) -> Vec<u8> {
    let mut v = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    v.extend_from_slice(&[0, 0, 0, 13, b'I', b'H', b'D', b'R']);
    v.extend_from_slice(&[0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0]);
    v.push(seed); // makes each fixture byte-distinguishable
    v
}

#[test]
fn an_added_signature_round_trips() {
    let dir = TempDir::new();
    let e = signatures::add(dir.path(), SignatureKind::Draw, &png(1), 1_000).expect("add");

    assert_eq!(e.kind, SignatureKind::Draw);
    assert_eq!(e.created_at, 1_000);
    assert_eq!(signatures::bytes(dir.path(), &e.id).expect("bytes"), png(1));

    let list = signatures::load(dir.path());
    assert_eq!(list.len(), 1);
    assert_eq!(list[0], e);
}

#[test]
fn blobs_live_beside_the_index_not_inside_it() {
    let dir = TempDir::new();
    let e = signatures::add(dir.path(), SignatureKind::Image, &png(2), 1).expect("add");

    assert!(blob_path(dir.path(), &e.id).is_file(), "blob written as its own file");
    let index = std::fs::read_to_string(index_path(dir.path())).expect("index");
    assert!(index.contains(&e.id));
    // The whole point of the split: the index stays small.
    assert!(!index.contains("PNG"), "blob bytes are not inlined: {index}");
}

#[test]
fn the_list_is_newest_first() {
    let dir = TempDir::new();
    let a = signatures::add(dir.path(), SignatureKind::Draw, &png(1), 1).expect("a");
    let b = signatures::add(dir.path(), SignatureKind::Type, &png(2), 2).expect("b");
    let c = signatures::add(dir.path(), SignatureKind::Image, &png(3), 3).expect("c");

    let ids: Vec<String> = signatures::load(dir.path()).into_iter().map(|e| e.id).collect();
    assert_eq!(ids, vec![c.id, b.id, a.id]);
}

#[test]
fn remove_deletes_the_entry_and_its_blob() {
    let dir = TempDir::new();
    let e = signatures::add(dir.path(), SignatureKind::Draw, &png(1), 1).expect("add");
    signatures::remove(dir.path(), &e.id).expect("remove");

    assert!(signatures::load(dir.path()).is_empty());
    assert!(!blob_path(dir.path(), &e.id).exists(), "blob unlinked");
    assert!(matches!(signatures::bytes(dir.path(), &e.id), Err(CommandError::NotFound(_))));
}

#[test]
fn removing_an_unknown_id_succeeds() {
    let dir = TempDir::new();
    // The end state the caller asked for is already true — not an error.
    signatures::remove(dir.path(), "no-such-id").expect("no-op remove");
}

#[test]
fn a_missing_library_reads_as_empty() {
    let dir = TempDir::new();
    assert!(signatures::load(dir.path()).is_empty());
}

#[test]
fn a_corrupt_index_reads_as_empty_rather_than_erroring() {
    let dir = TempDir::new();
    signatures::add(dir.path(), SignatureKind::Draw, &png(1), 1).expect("add");
    std::fs::write(index_path(dir.path()), b"{ not json at all").expect("corrupt it");

    // A mangled settings file must never block the app.
    assert!(signatures::load(dir.path()).is_empty());
}

#[test]
fn a_wrong_version_index_reads_as_empty() {
    let dir = TempDir::new();
    std::fs::write(index_path(dir.path()), br#"{"version":99,"entries":[]}"#).expect("write");
    assert!(signatures::load(dir.path()).is_empty());
}

#[test]
fn one_damaged_blob_does_not_take_down_the_library() {
    let dir = TempDir::new();
    let a = signatures::add(dir.path(), SignatureKind::Draw, &png(1), 1).expect("a");
    let b = signatures::add(dir.path(), SignatureKind::Draw, &png(2), 2).expect("b");
    std::fs::remove_file(blob_path(dir.path(), &a.id)).expect("nuke a's blob");

    // The index still lists both, and b is still readable.
    assert_eq!(signatures::load(dir.path()).len(), 2);
    assert_eq!(signatures::bytes(dir.path(), &b.id).expect("b readable"), png(2));
    assert!(matches!(signatures::bytes(dir.path(), &a.id), Err(CommandError::NotFound(_))));
}

#[test]
fn the_cap_prunes_the_oldest_and_deletes_its_blob() {
    let dir = TempDir::new();
    let first = signatures::add(dir.path(), SignatureKind::Draw, &png(0), 0).expect("first");
    for i in 1..=MAX_SIGNATURES {
        signatures::add(dir.path(), SignatureKind::Draw, &png(i as u8), i as u64).expect("add");
    }

    let list = signatures::load(dir.path());
    assert_eq!(list.len(), MAX_SIGNATURES);
    assert!(!list.iter().any(|e| e.id == first.id), "oldest pruned from the index");
    assert!(!blob_path(dir.path(), &first.id).exists(), "and its blob unlinked");
}

#[test]
fn non_png_bytes_are_rejected() {
    let dir = TempDir::new();
    let err = signatures::add(dir.path(), SignatureKind::Draw, b"GIF89a not a png", 1)
        .unwrap_err();
    assert!(matches!(err, CommandError::InvalidInput(_)), "got {err:?}");
    // And nothing was written on the way to failing.
    assert!(signatures::load(dir.path()).is_empty());
}

#[test]
fn the_library_survives_a_restart() {
    let dir = TempDir::new();
    let a = signatures::add(dir.path(), SignatureKind::Type, &png(7), 5).expect("add");

    // "Restart" = drop every in-memory handle and read the directory afresh.
    let reloaded = signatures::load(dir.path());
    assert_eq!(reloaded.len(), 1);
    assert_eq!(reloaded[0].id, a.id);
    assert_eq!(reloaded[0].kind, SignatureKind::Type);
    assert_eq!(signatures::bytes(dir.path(), &a.id).expect("bytes"), png(7));
}
