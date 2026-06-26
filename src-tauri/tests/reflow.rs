//! Integration tests for the in-place text-edit primitive (P4.A3).
//!
//! SPEC: P4-EDIT-001 (edit existing text) — verify by re-extracting that the run's
//! text actually changed in the saved file. Each test mutates bytes via `reflow`,
//! then reads the result back through the actor's A1 text-run extraction — the same
//! path the frontend would use to confirm an edit. (Delete + true redaction are
//! deferred: our bundled PDFium's `FPDFPage_RemoveObject` SIGSEGVs — see BACKLOG.)

use std::path::PathBuf;

use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Document, Object, Stream};

use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::document::open_pdf;
use vibepdf_lib::pdf::reflow::{replace_text_run, ReplaceTextRunEdit};
use vibepdf_lib::pdf::text_extract::{extract_text_runs, TextRun};
use vibepdf_lib::pdf::undo::Edit;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn temp_pdf(bytes: &[u8]) -> PathBuf {
    let p = std::env::temp_dir().join(format!("vibepdf-reflow-{}.pdf", uuid::Uuid::new_v4()));
    std::fs::write(&p, bytes).expect("write temp pdf");
    p
}

/// Read a byte buffer's page-0 text runs through the actor (the real A1 path).
async fn page0_runs(bytes: &[u8]) -> Vec<TextRun> {
    let path = temp_pdf(bytes);
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, path, None).expect("spawn");
    let runs = handle.read_text_runs(0).await.expect("read runs");
    drop(handle);
    runs
}

fn joined(runs: &[TextRun]) -> String {
    runs.iter().map(|r| r.text.as_str()).collect()
}

/// A one-page PDF whose text uses a non-embedded, non-base-14 font (Calibri) —
/// the case that drives the recreate-with-substitute branch.
fn non_embedded_calibri_pdf() -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Calibri",
    });
    let resources_id = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font_id } });
    let content = Content {
        operations: vec![
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["F1".into(), 24.into()]),
            Operation::new("Td", vec![72.into(), 700.into()]),
            Operation::new("Tj", vec![Object::string_literal("Original Calibri")]),
            Operation::new("ET", vec![]),
        ],
    };
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().expect("encode")));
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id, "Contents" => content_id,
        "Resources" => resources_id, "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1 }),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("serialize calibri pdf");
    out
}

#[tokio::test]
async fn replace_preserves_position_and_changes_text() {
    let original = std::fs::read(fixture("hello.pdf")).expect("read hello.pdf");
    let orig_runs = page0_runs(&original).await;
    assert!(joined(&orig_runs).contains("VibePDF"), "fixture sanity: {:?}", joined(&orig_runs));
    let orig_bbox = orig_runs[0].bbox;

    let edited = replace_text_run(&original, 0, 0, "Hello, World!").expect("replace");
    let new_runs = page0_runs(&edited).await;
    let text = joined(&new_runs);

    // SPEC: P4-EDIT-001 — the text changed and the old text is gone.
    assert!(text.contains("Hello, World!"), "new text present: {text:?}");
    assert!(!text.contains("VibePDF"), "old text gone: {text:?}");

    // ...and the run kept its place (set_text preserves the text matrix).
    let new_run = new_runs
        .iter()
        .find(|r| r.text.contains("World"))
        .expect("edited run present");
    assert!((new_run.bbox[0] - orig_bbox[0]).abs() < 2.0, "x preserved: {:?} vs {:?}", new_run.bbox, orig_bbox);
    assert!((new_run.bbox[1] - orig_bbox[1]).abs() < 2.0, "y preserved: {:?} vs {:?}", new_run.bbox, orig_bbox);
}

#[tokio::test]
async fn output_round_trips_through_pdfium() {
    // The edited bytes must open cleanly in PDFium (the project's "no silent
    // breakage" rule). Spawning the actor *is* that reload; assert the page survived.
    let original = std::fs::read(fixture("hello.pdf")).expect("read hello.pdf");
    let edited = replace_text_run(&original, 0, 0, "Round trip").expect("replace");

    let path = temp_pdf(&edited);
    let handle = DocumentActorHandle::spawn(None, uuid::Uuid::new_v4(), path, None).expect("reopen edited");
    assert_eq!(handle.page_count().await.expect("page count"), 1, "page count stable after edit");
    drop(handle);
}

#[tokio::test]
async fn replace_works_on_non_embedded_font() {
    let original = non_embedded_calibri_pdf();
    let before = page0_runs(&original).await;
    assert!(joined(&before).contains("Calibri"), "fixture sanity: {:?}", joined(&before));

    // Editing succeeds even when the font isn't embedded. A3 keeps the edit (the
    // font reference is preserved); A2's once-per-document banner already warns the
    // user that such text may render in a substitute. Baking in the substitute face
    // needs object removal, which is deferred (see BACKLOG / module docs).
    let edited = replace_text_run(&original, 0, 0, "Now Edited").expect("replace");
    let runs = page0_runs(&edited).await;
    assert!(joined(&runs).contains("Now Edited"), "edited text present: {:?}", joined(&runs));
    assert!(!joined(&runs).contains("Original"), "old text gone: {:?}", joined(&runs));
}

#[tokio::test]
async fn bad_page_or_run_index_errors() {
    let original = std::fs::read(fixture("hello.pdf")).expect("read hello.pdf");
    assert!(replace_text_run(&original, 99, 0, "x").is_err(), "out-of-range page rejected");
    assert!(replace_text_run(&original, 0, 99, "x").is_err(), "out-of-range run rejected");
}

/// The undo contract: applying `ReplaceTextRunEdit` mutates the live doc and the
/// returned inverse restores the original text exactly.
#[tokio::test]
async fn replace_edit_inverse_restores_original() {
    let original = std::fs::read(fixture("hello.pdf")).expect("read hello.pdf");
    let path = temp_pdf(&original);
    let (mut doc, _meta) = open_pdf(&path, None).expect("open");

    let edit = Box::new(ReplaceTextRunEdit { page: 0, run_index: 0, new_text: "Edited!".to_owned() });
    let inverse = edit.apply(&mut doc).expect("apply");
    assert!(extract_text_runs(&doc, 0).expect("extract").iter().any(|r| r.text.contains("Edited!")));

    inverse.apply(&mut doc).expect("apply inverse");
    let restored = joined(&extract_text_runs(&doc, 0).expect("extract"));
    assert!(restored.contains("VibePDF"), "inverse restores original: {restored:?}");

    // Drop the live document (mirrors `open_document_metadata`'s own drop).
    drop(doc);
}

/// Writes an edited `hello.pdf` to /tmp for the manual three-reader check. Ignored
/// by default (produces an artifact); run with `--ignored`.
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn writes_verification_artifact() {
    let original = std::fs::read(fixture("hello.pdf")).expect("read hello.pdf");
    let edited = replace_text_run(&original, 0, 0, "Hello, World!").expect("replace");
    std::fs::write("/tmp/vibepdf-verify.pdf", &edited).expect("write artifact");
}
