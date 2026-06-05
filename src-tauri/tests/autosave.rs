//! Integration tests for autosave write/scan/discard (P2.A2).
//!
//! SPEC: infrastructure — `docs/04` § "Saving and auto-save". The live
//! 30s tick and the dirty-only write are dormant in P2.A2 (nothing makes
//! a document dirty yet), so these drive the underlying functions
//! directly against a real PDFium document — the same approach
//! `save_noop.rs` takes for the save path. The end-to-end crash-recovery
//! demo lands with P2.B2 (the first edit that dirties a document).

use std::path::PathBuf;

use vibepdf_lib::pdf::autosave::{discard_autosave, scan_autosaves, write_autosave};
use vibepdf_lib::pdf::document::open_pdf;

fn hello_pdf() -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic/hello.pdf");
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn temp_subdir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("vibepdf-autosave-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

#[test]
fn write_then_scan_round_trips() {
    let dir = temp_subdir();
    let (doc, _meta) = open_pdf(&hello_pdf(), None).expect("open fixture");

    let pdf_path = write_autosave(&doc, &dir, "doc-123", "/Users/me/foo.pdf")
        .expect("write_autosave");
    drop(doc);

    assert!(dir.join("doc-123.pdf").is_file());
    assert!(dir.join("doc-123.json").is_file());

    // The autosaved copy is a valid PDF that re-opens in PDFium.
    let (doc2, meta2) = open_pdf(&pdf_path, None).expect("autosave copy re-opens");
    assert_eq!(meta2.page_count, 1);
    drop(doc2);

    // scan surfaces it with the original path preserved.
    let entries = scan_autosaves(&dir).expect("scan");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].document_id, "doc-123");
    assert_eq!(entries[0].original_path, "/Users/me/foo.pdf");
    assert_eq!(
        entries[0].autosave_path,
        pdf_path.to_string_lossy().into_owned()
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn discard_removes_pdf_and_sidecar_idempotently() {
    let dir = temp_subdir();
    let (doc, _m) = open_pdf(&hello_pdf(), None).expect("open fixture");
    write_autosave(&doc, &dir, "gone", "/x.pdf").expect("write_autosave");
    drop(doc);
    assert!(dir.join("gone.pdf").is_file());

    discard_autosave(&dir, "gone").expect("discard");
    assert!(!dir.join("gone.pdf").exists());
    assert!(!dir.join("gone.json").exists());

    // Discarding again is a no-op, not an error.
    discard_autosave(&dir, "gone").expect("discard again");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn scan_skips_orphaned_and_malformed_entries() {
    let dir = temp_subdir();
    // Valid sidecar but no matching .pdf → orphaned, skip.
    std::fs::write(
        dir.join("orphan.json"),
        br#"{"documentId":"orphan","originalPath":"/a.pdf","savedAt":1}"#,
    )
    .unwrap();
    // Unparseable JSON → skip.
    std::fs::write(dir.join("bad.json"), b"{not json").unwrap();
    // Non-json file → ignored.
    std::fs::write(dir.join("note.txt"), b"hi").unwrap();

    let entries = scan_autosaves(&dir).expect("scan");
    assert!(entries.is_empty(), "orphaned + malformed entries must be skipped");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn scan_missing_dir_is_empty() {
    let dir = temp_subdir();
    std::fs::remove_dir_all(&dir).expect("remove the dir so it is absent");
    assert!(scan_autosaves(&dir).expect("scan absent dir").is_empty());
}
