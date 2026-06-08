//! Integration tests for merge (P2.C4).
//!
//! SPEC: P2-PAGE-008 — concatenation + annotations + **bookmarks + form
//! fields** (colliding `/T` names suffixed). Merge is a standalone op (no
//! actor), so these call `pdf::merge::merge_documents` directly against
//! fixtures. The whole binary runs single-threaded (PDFium), so the inspection
//! helpers don't need to hold the crate-private `PDFIUM_LOCK`.

use std::path::{Path, PathBuf};

use vibepdf_lib::pdf::cos::read_form_field_names;
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

/// Count the document's top-level bookmarks via PDFium (proves the outline
/// survived the lopdf-merge → PDFium load+save round-trip).
fn top_level_bookmark_count(path: &Path) -> usize {
    let (doc, _meta) = open_pdf(path, None).expect("open");
    let mut n = 0;
    let mut node = doc.bookmarks().root();
    while let Some(b) = node {
        n += 1;
        node = b.next_sibling();
    }
    n
}

/// The merged document's top-level form-field names (read via the COS layer).
fn field_names(path: &Path) -> Vec<String> {
    let bytes = std::fs::read(path).expect("read output");
    read_form_field_names(&bytes).expect("read field names")
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
    assert!(format!("{err}").contains("cannot read"), "got: {err}");
    assert!(!out.exists(), "a failed merge must not write a file");

    let _ = std::fs::remove_dir_all(&dir);
}

/// SPEC: P2-PAGE-008 — the merged document preserves each source's bookmarks
/// (one outline subtree per source). Merging `bookmarks.pdf` (3 top-level
/// bookmarks) with itself yields 6 top-level bookmarks.
#[test]
fn merge_carries_bookmarks() {
    let dir = temp_dir();
    let out = dir.join("merged.pdf");

    merge_documents(&[fixture("bookmarks.pdf"), fixture("bookmarks.pdf")], &out).expect("merge");
    assert_eq!(page_count(&out), 12, "6 + 6 pages");
    assert_eq!(top_level_bookmark_count(&out), 6, "both sources' bookmarks preserved");

    let _ = std::fs::remove_dir_all(&dir);
}

/// SPEC: P2-PAGE-008 — the merged document preserves form fields, and a
/// colliding field name is suffixed (`name` → `name_2`). Merging `forms.pdf`
/// (one field `name`) with itself yields fields `name` and `name_2`.
#[test]
fn merge_carries_form_fields_with_rename() {
    let dir = temp_dir();
    let out = dir.join("merged.pdf");

    merge_documents(&[fixture("forms.pdf"), fixture("forms.pdf")], &out).expect("merge");
    assert_eq!(page_count(&out), 2);

    let mut names = field_names(&out);
    names.sort();
    assert_eq!(names, vec!["name".to_string(), "name_2".to_string()], "collision renamed");

    let _ = std::fs::remove_dir_all(&dir);
}

/// Writes a merged PDF to `/tmp/vibepdf-verify-merged.pdf` for the manual
/// cross-reader check. Ignored — run on demand:
///   cargo test --test merge merge_writes_verification_artifact -- --ignored
#[test]
#[ignore = "produces a verification artifact; run on demand"]
fn merge_writes_verification_artifact() {
    let out = PathBuf::from("/tmp/vibepdf-verify-merged.pdf");
    // bookmarks.pdf (6 pp, 3 bookmarks) + forms.pdf (1 pp, a form field) →
    // a single file exercising bookmarks AND a form field.
    merge_documents(&[fixture("bookmarks.pdf"), fixture("forms.pdf")], &out).expect("merge");
    assert!(out.is_file());
    assert_eq!(page_count(&out), 7);
    assert_eq!(top_level_bookmark_count(&out), 3, "bookmarks preserved");
    eprintln!("wrote merged verification artifact to {}", out.display());
}
