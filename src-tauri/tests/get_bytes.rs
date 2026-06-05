//! Integration test for the edit-preview byte source (`pdf_get_bytes` /
//! `Message::GetBytes`). The frontend reloads PDF.js from these bytes so
//! the main view reflects *in-memory* edits without a save/reopen — so the
//! key property is that the bytes carry an unsaved edit.

use std::path::PathBuf;

use pdfium_render::prelude::PdfPageRenderRotation;
use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::document::open_pdf;

fn hello_pdf() -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic/hello.pdf");
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn temp_subdir() -> PathBuf {
    let d = std::env::temp_dir().join(format!("vibepdf-getbytes-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).expect("create temp dir");
    d
}

#[tokio::test]
async fn get_bytes_reopens_with_same_page_count() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, hello_pdf(), None).expect("spawn");

    let bytes = handle.get_bytes().await.expect("get_bytes");
    assert!(!bytes.is_empty(), "serialized document must be non-empty");

    // `open_pdf` loads from a path, so stage the bytes to a temp file.
    let dir = temp_subdir();
    let p = dir.join("bytes.pdf");
    std::fs::write(&p, &bytes).expect("write bytes");
    let (doc, meta) = open_pdf(&p, None).expect("bytes reopen as a PDF");
    assert_eq!(meta.page_count, 1);
    drop(doc);

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn get_bytes_reflects_an_in_memory_rotation() {
    // The whole point of the pipeline: the bytes carry an *unsaved* edit.
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, hello_pdf(), None).expect("spawn");

    handle.rotate_pages(vec![0], 1).await.expect("rotate 90");
    let bytes = handle.get_bytes().await.expect("get_bytes after rotate");

    let dir = temp_subdir();
    let p = dir.join("rotated-bytes.pdf");
    std::fs::write(&p, &bytes).expect("write bytes");
    let (doc, _m) = open_pdf(&p, None).expect("reopen");
    let rotation = doc
        .pages()
        .get(0)
        .expect("page 0")
        .rotation()
        .expect("rotation");
    drop(doc);
    assert_eq!(
        rotation,
        PdfPageRenderRotation::Degrees90,
        "get_bytes must reflect the unsaved in-memory rotation"
    );

    drop(handle);
    let _ = std::fs::remove_dir_all(&dir);
}
