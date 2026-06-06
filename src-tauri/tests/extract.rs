//! Integration tests for extract pages (P2.C2).
//!
//! SPEC: P2-PAGE-006 — a new PDF with exactly the selected pages, resources
//! copied. Exercised through the actor against `links.pdf` (3 pages). The
//! output is opened back through PDFium to prove it's structurally valid
//! (no dangling indirect refs); visual glyph fidelity is the cross-reader
//! ritual.

use std::path::{Path, PathBuf};

use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::document::open_pdf;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn temp_subdir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("vibepdf-extract-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

fn page_count(path: &Path) -> u32 {
    let (doc, meta) = open_pdf(path, None).expect("open output");
    let n = meta.page_count;
    drop(doc);
    n
}

#[tokio::test]
async fn extract_produces_selected_pages() {
    let dir = temp_subdir();
    let out = dir.join("extracted.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("links.pdf"), None).expect("spawn");

    // Extract page 1 and page 3 (0-based [0, 2]).
    let outcome = handle.extract_pages(vec![0, 2], out.clone()).await.expect("extract");
    assert!(!outcome.no_op);
    assert!(outcome.bytes_written > 0);
    assert_eq!(page_count(&out), 2, "output should contain exactly the two pages");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn extract_single_and_all() {
    let dir = temp_subdir();
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("links.pdf"), None).expect("spawn");

    let one = dir.join("one.pdf");
    handle.extract_pages(vec![1], one.clone()).await.expect("extract one");
    assert_eq!(page_count(&one), 1);

    let all = dir.join("all.pdf");
    handle.extract_pages(vec![0, 1, 2], all.clone()).await.expect("extract all");
    assert_eq!(page_count(&all), 3);

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn extract_out_of_range_and_empty_write_nothing() {
    let dir = temp_subdir();
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("links.pdf"), None).expect("spawn");

    let out = dir.join("nope.pdf");
    let err = handle.extract_pages(vec![99], out.clone()).await.expect_err("out of range");
    assert!(format!("{err}").contains("out of range"), "got: {err}");
    assert!(!out.exists(), "a failed extract must not write a file");

    let err2 = handle.extract_pages(vec![], out.clone()).await.expect_err("empty");
    assert!(format!("{err2}").contains("no pages"), "got: {err2}");
    assert!(!out.exists());

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Writes an extracted PDF to `/tmp/vibepdf-verify-extracted.pdf` for the
/// manual cross-reader check. Ignored — run on demand:
///   cargo test --test extract extract_writes_verification_artifact -- --ignored
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn extract_writes_verification_artifact() {
    let out = PathBuf::from("/tmp/vibepdf-verify-extracted.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("links.pdf"), None).expect("spawn");

    handle.extract_pages(vec![0, 2], out.clone()).await.expect("extract");
    assert!(out.is_file());
    assert_eq!(page_count(&out), 2);
    eprintln!("wrote extracted verification artifact to {}", out.display());
}
