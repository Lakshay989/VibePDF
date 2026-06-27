//! Integration tests for adding a text box as page content (P4.B2).
//!
//! SPEC: P4-EDIT-003 — new text is added to the page **content stream, not as an
//! annotation**. So the proof is: after the add, A1 text extraction finds the new
//! text *as a run*, and the page's annotation count is unchanged.

use std::path::PathBuf;

use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Document, Object, Stream};

use vibepdf_lib::pdf::actor::DocumentActorHandle;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn temp_pdf(bytes: &[u8]) -> PathBuf {
    let p = std::env::temp_dir().join(format!("vibepdf-add-{}.pdf", uuid::Uuid::new_v4()));
    std::fs::write(&p, bytes).expect("write temp");
    p
}

async fn page0_text(bytes: &[u8]) -> String {
    let handle = DocumentActorHandle::spawn(None, uuid::Uuid::new_v4(), temp_pdf(bytes), None).expect("spawn");
    let runs = handle.read_text_runs(0).await.expect("read runs");
    drop(handle);
    runs.iter().map(|r| r.text.as_str()).collect()
}

async fn page0_annot_count(bytes: &[u8]) -> usize {
    let handle = DocumentActorHandle::spawn(None, uuid::Uuid::new_v4(), temp_pdf(bytes), None).expect("spawn");
    let annots = handle.read_annotations().await.expect("read annots");
    drop(handle);
    annots.iter().filter(|a| a.page == 0).count()
}

const RECT: [f32; 4] = [100.0, 400.0, 400.0, 500.0];

fn add(bytes: &[u8], text: &str) -> Vec<u8> {
    use vibepdf_lib::pdf::cos::add_text_box;
    add_text_box(bytes, 0, RECT, text, "Helvetica", 18.0, "#102030", false, false, false).expect("add text box")
}

#[tokio::test]
async fn adds_text_to_the_content_stream() {
    let original = std::fs::read(fixture("hello.pdf")).expect("read hello.pdf");
    let edited = add(&original, "AddedRun");

    // SPEC: P4-EDIT-003 — the new text extracts as a content run (not an annotation).
    let text = page0_text(&edited).await;
    assert!(text.contains("AddedRun"), "added text is a content run: {text:?}");
    assert!(text.contains("VibePDF"), "original content survives: {text:?}");
}

#[tokio::test]
async fn does_not_add_an_annotation() {
    let original = std::fs::read(fixture("hello.pdf")).expect("read hello.pdf");
    let before = page0_annot_count(&original).await;
    let edited = add(&original, "NotAnAnnot");
    assert_eq!(page0_annot_count(&edited).await, before, "no annotation was created");
}

/// A page whose /Resources /Font already defines `F1` — the add must pick a fresh
/// name and leave the original text intact.
fn pdf_with_f1_font() -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let resources_id = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font_id } });
    let content = Content {
        operations: vec![
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["F1".into(), 24.into()]),
            Operation::new("Td", vec![72.into(), 700.into()]),
            Operation::new("Tj", vec![Object::string_literal("ORIGINAL")]),
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
    doc.save_to(&mut out).expect("serialize");
    out
}

#[tokio::test]
async fn unique_font_resource_no_collision() {
    let original = pdf_with_f1_font();
    assert_eq!(page0_text(&original).await, "ORIGINAL", "fixture sanity");

    let edited = add(&original, "FreshFont");
    let text = page0_text(&edited).await;
    assert!(text.contains("ORIGINAL"), "existing F1 text intact: {text:?}");
    assert!(text.contains("FreshFont"), "new text rendered: {text:?}");
}

#[tokio::test]
async fn empty_rect_or_text_errors() {
    use vibepdf_lib::pdf::cos::add_text_box;
    let original = std::fs::read(fixture("hello.pdf")).expect("read hello.pdf");
    assert!(
        add_text_box(&original, 0, [10.0, 10.0, 10.0, 10.0], "x", "Helvetica", 18.0, "#000000", false, false, false).is_err(),
        "empty rect rejected"
    );
    assert!(
        add_text_box(&original, 0, RECT, "   ", "Helvetica", 18.0, "#000000", false, false, false).is_err(),
        "blank text rejected"
    );
}

#[tokio::test]
async fn actor_add_then_undo() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");

    let state = handle
        .add_text_box(0, RECT, "ViaActor".to_owned(), "Helvetica".to_owned(), 18.0, "#000000".to_owned(), false, false, false)
        .await
        .expect("add text box");
    assert!(state.can_undo, "adding text is undoable");

    let joined: String = handle.read_text_runs(0).await.expect("runs").iter().map(|r| r.text.clone()).collect();
    assert!(joined.contains("ViaActor"), "added text present after actor add: {joined:?}");

    handle.undo().await.expect("undo");
    let after: String = handle.read_text_runs(0).await.expect("runs").iter().map(|r| r.text.clone()).collect();
    assert!(!after.contains("ViaActor"), "undo removes the added text: {after:?}");
    drop(handle);
}

#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn writes_verification_artifact() {
    let original = std::fs::read(fixture("hello.pdf")).expect("read hello.pdf");
    let edited = add(&original, "Added by VibePDF");
    std::fs::write("/tmp/vibepdf-verify.pdf", &edited).expect("write artifact");
}
