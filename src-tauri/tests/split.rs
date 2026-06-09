//! Integration tests for split (P2.C3).
//!
//! SPEC: P2-PAGE-007 — four modes: every N pages, at specific pages, by
//! file-size target, by top-level bookmarks. Each produces N output files.
//! Exercised through the actor against `links.pdf` (3 pages, no bookmarks)
//! and `bookmarks.pdf` (6 pages, 3 top-level bookmarks → pages 0/2/4). Each
//! output is reopened through PDFium to prove it is structurally valid.

use std::path::{Path, PathBuf};

use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::document::open_pdf;
use vibepdf_lib::pdf::split::SplitMode;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn temp_subdir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("vibepdf-split-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

fn page_count(path: &Path) -> u32 {
    let (doc, meta) = open_pdf(path, None).expect("open output");
    let n = meta.page_count;
    drop(doc);
    n
}

/// Output files in `dir`, sorted by name.
fn output_files(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("read dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("pdf"))
        .collect();
    v.sort();
    v
}

#[tokio::test]
async fn split_every_n_produces_expected_files() {
    let dir = temp_subdir();
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("links.pdf"), None).expect("spawn");

    let outcome = handle
        .split_document(SplitMode::EveryN { n: 2 }, dir.clone(), "doc".into())
        .await
        .expect("split every 2");

    assert_eq!(outcome.files.len(), 2, "3 pages / 2 → 2 files");
    let files = output_files(&dir);
    assert_eq!(files.len(), 2);
    assert_eq!(page_count(&files[0]), 2);
    assert_eq!(page_count(&files[1]), 1);

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn split_at_pages_produces_expected_files() {
    let dir = temp_subdir();
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("links.pdf"), None).expect("spawn");

    // Split before page 2 (1-based) → [page 1][pages 2-3].
    let outcome = handle
        .split_document(SplitMode::AtPages { boundaries: vec![2] }, dir.clone(), "doc".into())
        .await
        .expect("split at page 2");

    assert_eq!(outcome.files.len(), 2);
    let files = output_files(&dir);
    assert_eq!(page_count(&files[0]), 1);
    assert_eq!(page_count(&files[1]), 2);

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn split_by_size_one_file_per_page() {
    let dir = temp_subdir();
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("links.pdf"), None).expect("spawn");

    // A 1-byte target is smaller than any single page, so every page lands
    // in its own file (a lone over-target page is never dropped).
    let outcome = handle
        .split_document(SplitMode::BySize { max_bytes: 1 }, dir.clone(), "doc".into())
        .await
        .expect("split by tiny size");

    assert_eq!(outcome.files.len(), 3, "each page its own file");
    for f in output_files(&dir) {
        assert_eq!(page_count(&f), 1);
    }

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn split_by_size_huge_target_is_one_file_error() {
    let dir = temp_subdir();
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("links.pdf"), None).expect("spawn");

    // A target larger than the whole document yields a single group, which
    // is not a "split" — the actor reports it rather than emitting one file.
    let err = handle
        .split_document(
            SplitMode::BySize { max_bytes: 50_000_000 },
            dir.clone(),
            "doc".into(),
        )
        .await
        .expect_err("one group is not a split");
    assert!(format!("{err}").contains("fewer than two"), "got: {err}");
    assert!(output_files(&dir).is_empty(), "no files on a non-split");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn split_by_bookmarks_produces_expected_files() {
    let dir = temp_subdir();
    let id = uuid::Uuid::new_v4();
    let handle =
        DocumentActorHandle::spawn(None, id, fixture("bookmarks.pdf"), None).expect("spawn");

    // Top-level bookmarks at pages 0, 2, 4 → three 2-page files.
    let outcome = handle
        .split_document(SplitMode::Bookmarks, dir.clone(), "doc".into())
        .await
        .expect("split by bookmarks");

    assert_eq!(outcome.files.len(), 3);
    let files = output_files(&dir);
    assert_eq!(files.len(), 3);
    for f in &files {
        assert_eq!(page_count(f), 2);
    }

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn split_rejects_bad_input() {
    let dir = temp_subdir();
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("links.pdf"), None).expect("spawn");

    // n == 0
    let e1 = handle
        .split_document(SplitMode::EveryN { n: 0 }, dir.clone(), "doc".into())
        .await
        .expect_err("n == 0");
    assert!(format!("{e1}").contains("≥ 1") || format!("{e1}").contains(">= 1"), "got: {e1}");

    // empty boundaries
    let e2 = handle
        .split_document(SplitMode::AtPages { boundaries: vec![] }, dir.clone(), "doc".into())
        .await
        .expect_err("empty boundaries");
    assert!(format!("{e2}").contains("no split points"), "got: {e2}");

    // out-of-range boundary (page 1 means "split before the first page")
    let e3 = handle
        .split_document(SplitMode::AtPages { boundaries: vec![1] }, dir.clone(), "doc".into())
        .await
        .expect_err("boundary out of range");
    assert!(format!("{e3}").contains("out of range"), "got: {e3}");

    // no bookmarks in links.pdf
    let e4 = handle
        .split_document(SplitMode::Bookmarks, dir.clone(), "doc".into())
        .await
        .expect_err("no bookmarks");
    assert!(format!("{e4}").contains("bookmark"), "got: {e4}");

    assert!(output_files(&dir).is_empty(), "no files written on any error");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

/// SPEC: P2-PAGE-003 (cleanup) — a link that crosses a split boundary dangles
/// in the output file and is pruned. `links.pdf` page 1 links to page 3; split
/// at page 2 puts page 1 in file 1 and page 3 in file 2, so file 1's link is
/// dangling and must be gone.
#[tokio::test]
async fn split_prunes_cross_file_link() {
    let dir = temp_subdir();
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("links.pdf"), None).expect("spawn");

    handle
        .split_document(SplitMode::AtPages { boundaries: vec![2] }, dir.clone(), "doc".into())
        .await
        .expect("split");

    let files = output_files(&dir);
    assert_eq!(files.len(), 2);
    // file 1 is [page 1]; its link to page 3 (now in file 2) is dangling.
    let (doc, _meta) = open_pdf(&files[0], None).expect("open file 1");
    let empty = {
        let pages = doc.pages();
        pages.get(0).expect("page 0").annotations().is_empty()
    };
    assert!(empty, "cross-file dangling link pruned");
    drop(doc);

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Writes split output to `/tmp/vibepdf-verify-split-*.pdf` for the manual
/// cross-reader check. Ignored — run on demand:
///   cargo test --test split split_writes_verification_artifacts -- --ignored
#[tokio::test]
#[ignore = "produces verification artifacts; run on demand"]
async fn split_writes_verification_artifacts() {
    let dir = PathBuf::from("/tmp");
    let id = uuid::Uuid::new_v4();
    let handle =
        DocumentActorHandle::spawn(None, id, fixture("bookmarks.pdf"), None).expect("spawn");

    let outcome = handle
        .split_document(SplitMode::EveryN { n: 2 }, dir.clone(), "vibepdf-verify-split".into())
        .await
        .expect("split");
    for f in &outcome.files {
        eprintln!("wrote split verification artifact to {}", f.path);
    }
    assert_eq!(outcome.files.len(), 3);
}
