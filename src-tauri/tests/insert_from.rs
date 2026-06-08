//! Integration tests for insert-from-PDF (P2.D1).
//!
//! SPEC: P2-PAGE-005 (PARTIAL) — content + page annotations + dimensions.
//! Form fields are deferred (lopdf). Exercised through the actor: open
//! `hello.pdf` (1 page) as the destination and import pages of `links.pdf`
//! (3 pages, a /Link annotation on page 1). Single-threaded (PDFium), so the
//! inspection helper doesn't hold the crate-private lock.

use std::path::{Path, PathBuf};

use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::cos::read_form_field_names;
use vibepdf_lib::pdf::document::{open_document_metadata, open_pdf};

/// The form-field names of a saved PDF (read via the COS layer).
fn field_names(path: &std::path::Path) -> Vec<String> {
    let bytes = std::fs::read(path).expect("read output");
    read_form_field_names(&bytes).expect("read field names")
}

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

/// Live page count via fresh metadata (the actor's cached count is open-time).
async fn live_count(handle: &DocumentActorHandle) -> u32 {
    handle.metadata_live().await.expect("metadata").page_count
}

fn page_has_annotations(path: &Path, page: i32) -> bool {
    let (doc, _meta) = open_pdf(path, None).expect("open");
    let pages = doc.pages();
    let p = pages.get(page).expect("page");
    !p.annotations().is_empty()
}

#[tokio::test]
async fn insert_from_adds_pages_and_is_undoable() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    // Insert all 3 pages of links.pdf after hello's only page (index 1).
    let hist = handle
        .insert_from_pdf(fixture("links.pdf"), vec![0, 1, 2], 1)
        .await
        .expect("insert");
    assert!(hist.can_undo);
    assert_eq!(live_count(&handle).await, 4, "1 + 3 inserted");

    handle.undo().await.expect("undo");
    assert_eq!(live_count(&handle).await, 1, "undo removes the inserted block");

    handle.redo().await.expect("redo");
    assert_eq!(live_count(&handle).await, 4, "redo restores it");

    drop(handle);
}

#[tokio::test]
async fn insert_from_preserves_annotations() {
    let dir = std::env::temp_dir().join(format!("vibepdf-insertfrom-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let out = dir.join("out.pdf");

    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    // Insert links.pdf page 1 (its annotated page) at index 1, then save and
    // reopen to confirm the annotation rode along.
    handle
        .insert_from_pdf(fixture("links.pdf"), vec![0], 1)
        .await
        .expect("insert");
    handle.save(Some(out.clone())).await.expect("save");

    assert!(page_has_annotations(&out, 1), "inserted page should keep its annotation");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn insert_from_preserves_form_fields() {
    let dir = std::env::temp_dir().join(format!("vibepdf-insertff-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let out = dir.join("out.pdf");

    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    // Insert forms.pdf (one text field "name") after hello's page.
    handle.insert_from_pdf(fixture("forms.pdf"), vec![0], 1).await.expect("insert");
    handle.save(Some(out.clone())).await.expect("save");

    let names = field_names(&out);
    assert!(names.iter().any(|n| n == "name"), "inserted form field preserved; got {names:?}");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn insert_from_renames_colliding_field() {
    let dir = std::env::temp_dir().join(format!("vibepdf-insertff2-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let out = dir.join("out.pdf");

    let id = uuid::Uuid::new_v4();
    // Destination already has a "name" field; inserting forms.pdf collides.
    let handle = DocumentActorHandle::spawn(None, id, fixture("forms.pdf"), None).expect("spawn");

    handle.insert_from_pdf(fixture("forms.pdf"), vec![0], 1).await.expect("insert");
    handle.save(Some(out.clone())).await.expect("save");

    let mut names = field_names(&out);
    names.sort();
    assert_eq!(names, vec!["name".to_string(), "name_2".to_string()], "collision renamed");

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn insert_from_at_start_and_end() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    handle.insert_from_pdf(fixture("links.pdf"), vec![0], 0).await.expect("at start");
    assert_eq!(live_count(&handle).await, 2);

    let end = live_count(&handle).await as i32;
    handle.insert_from_pdf(fixture("links.pdf"), vec![2], end).await.expect("at end");
    assert_eq!(live_count(&handle).await, 3);

    drop(handle);
}

#[tokio::test]
async fn insert_from_validates() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    // Source page out of range.
    let e1 = handle
        .insert_from_pdf(fixture("links.pdf"), vec![99], 0)
        .await
        .expect_err("bad source page");
    assert!(format!("{e1}").contains("out of range"), "got: {e1}");

    // Insert index out of range (hello has 1 page → valid 0..=1).
    let e2 = handle
        .insert_from_pdf(fixture("links.pdf"), vec![0], 5)
        .await
        .expect_err("bad index");
    assert!(format!("{e2}").contains("out of range"), "got: {e2}");

    assert_eq!(live_count(&handle).await, 1, "no mutation on a rejected insert");

    drop(handle);
}

#[tokio::test]
async fn insert_from_missing_source_errors() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    let missing = PathBuf::from("/no/such/file-xyz.pdf");
    let err = handle.insert_from_pdf(missing, vec![0], 0).await.expect_err("missing source");
    assert!(format!("{err}").contains("cannot open"), "got: {err}");
    assert_eq!(live_count(&handle).await, 1, "no mutation when the source can't open");

    drop(handle);
}

#[test]
fn peek_page_count_reports_count() {
    let meta = open_document_metadata(&fixture("links.pdf")).expect("metadata");
    assert_eq!(meta.page_count, 3);
}

/// Writes an insert-from result to `/tmp/vibepdf-verify-insertfrom.pdf` for the
/// manual cross-reader check. Ignored — run on demand:
///   cargo test --test insert_from insert_from_writes_verification_artifact -- --ignored
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn insert_from_writes_verification_artifact() {
    let out = PathBuf::from("/tmp/vibepdf-verify-insertfrom.pdf");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    // Insert links.pdf (annotated) then forms.pdf (a form field) so the
    // artifact exercises both annotations and a fillable form field.
    handle.insert_from_pdf(fixture("links.pdf"), vec![0, 1, 2], 1).await.expect("insert links");
    handle.insert_from_pdf(fixture("forms.pdf"), vec![0], 4).await.expect("insert form");
    handle.save(Some(out.clone())).await.expect("save");
    assert!(out.is_file());
    eprintln!("wrote insert-from verification artifact to {}", out.display());

    drop(handle);
}
