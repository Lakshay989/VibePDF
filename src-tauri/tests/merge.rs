//! Integration tests for merge (P2.C4).
//!
//! SPEC: P2-PAGE-008 (PARTIAL) — concatenation + page-annotation preservation.
//! Bookmarks and form fields are deferred (need lopdf); the
//! `merge_does_not_yet_carry_bookmarks` test deliberately locks that current
//! behavior so the follow-up trips it. Merge is a standalone op (no actor), so
//! these call `pdf::merge::merge_documents` directly against fixtures. The
//! whole binary runs single-threaded (PDFium), so the inspection helpers don't
//! need to hold the crate-private `PDFIUM_LOCK`.

use std::path::{Path, PathBuf};

use vibepdf_lib::pdf::document::open_pdf;
use vibepdf_lib::pdf::merge::merge_documents;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn temp_dir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("vibepdf-merge-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

fn page_count(path: &Path) -> u32 {
    let (doc, meta) = open_pdf(path, None).expect("open output");
    let n = meta.page_count;
    drop(doc);
    n
}

/// Whether the given 0-based page of a PDF file carries any annotations.
fn page_has_annotations(path: &Path, page: i32) -> bool {
    let (doc, _meta) = open_pdf(path, None).expect("open");
    let pages = doc.pages();
    let p = pages.get(page).expect("page");
    !p.annotations().is_empty()
}

#[test]
fn merge_concatenates_page_counts() {
    let dir = temp_dir();
    let out = dir.join("merged.pdf");

    // hello.pdf (1 page) + links.pdf (3 pages) → 4 pages.
    let outcome =
        merge_documents(&[fixture("hello.pdf"), fixture("links.pdf")], &out).expect("merge");
    assert!(!outcome.no_op);
    assert!(outcome.bytes_written > 0);
    assert_eq!(page_count(&out), 4);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn merge_preserves_page_annotations() {
    let dir = temp_dir();
    let out = dir.join("merged.pdf");

    // links.pdf page 1 carries a /Link annotation; it must survive import.
    merge_documents(&[fixture("links.pdf"), fixture("hello.pdf")], &out).expect("merge");
    assert!(page_has_annotations(&out, 0), "page-1 link annotation should survive merge");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn merge_order_is_respected() {
    let dir = temp_dir();

    // links first → output page 0 is links' annotated page 1.
    let a = dir.join("links-first.pdf");
    merge_documents(&[fixture("links.pdf"), fixture("hello.pdf")], &a).expect("merge a");
    assert!(page_has_annotations(&a, 0), "links-first: page 0 should be annotated");

    // hello first → output page 0 is hello's unannotated page.
    let b = dir.join("hello-first.pdf");
    merge_documents(&[fixture("hello.pdf"), fixture("links.pdf")], &b).expect("merge b");
    assert!(!page_has_annotations(&b, 0), "hello-first: page 0 should be unannotated");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn merge_requires_at_least_two() {
    let dir = temp_dir();
    let out = dir.join("nope.pdf");

    let err = merge_documents(&[fixture("hello.pdf")], &out).expect_err("one file");
    assert!(format!("{err}").contains("at least two"), "got: {err}");
    assert!(!out.exists(), "a rejected merge must not write a file");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn merge_missing_file_errors() {
    let dir = temp_dir();
    let out = dir.join("nope.pdf");
    let missing = dir.join("does-not-exist.pdf");

    let err = merge_documents(&[fixture("hello.pdf"), missing], &out).expect_err("missing input");
    assert!(format!("{err}").contains("cannot open"), "got: {err}");
    assert!(!out.exists(), "a failed merge must not write a file");

    let _ = std::fs::remove_dir_all(&dir);
}

/// Locks the CURRENT (deferred) behavior: `FPDF_ImportPages` does not copy the
/// `/Outlines` tree, so a merge of bookmarked PDFs has no top-level bookmarks.
/// When the lopdf follow-up lands bookmark preservation, this test SHOULD fail
/// and be updated — that's the signal the gap closed.
#[test]
fn merge_does_not_yet_carry_bookmarks() {
    let dir = temp_dir();
    let out = dir.join("merged.pdf");

    merge_documents(&[fixture("bookmarks.pdf"), fixture("hello.pdf")], &out).expect("merge");

    let (doc, _meta) = open_pdf(&out, None).expect("open output");
    assert!(
        doc.bookmarks().root().is_none(),
        "bookmark preservation is deferred (lopdf); update this test when it lands"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Writes a merged PDF to `/tmp/vibepdf-verify-merged.pdf` for the manual
/// cross-reader check. Ignored — run on demand:
///   cargo test --test merge merge_writes_verification_artifact -- --ignored
#[test]
#[ignore = "produces a verification artifact; run on demand"]
fn merge_writes_verification_artifact() {
    let out = PathBuf::from("/tmp/vibepdf-verify-merged.pdf");
    merge_documents(
        &[fixture("bookmarks.pdf"), fixture("links.pdf"), fixture("hello.pdf")],
        &out,
    )
    .expect("merge");
    assert!(out.is_file());
    // 6 + 3 + 1 = 10 pages.
    assert_eq!(page_count(&out), 10);
    eprintln!("wrote merged verification artifact to {}", out.display());
}
