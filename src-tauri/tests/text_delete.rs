//! Integration tests for content-stream text deletion (P4.B3 / P6-SEC-010).
//!
//! SPEC: P4-EDIT-004 (remove a run from the content stream) and P6-SEC-010(a)+(c)
//! (true removal, verified by re-extraction). The primitive splices the run's show
//! operator out at the lopdf level and verifies via PDFium that exactly the target
//! run is gone.

use std::path::PathBuf;

use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Document, Object, Stream};

use vibepdf_lib::pdf::reflow::delete_text_run;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

/// Run page-0's text through PDFium (a throwaway-doc load via the actor read path).
async fn page0_text(bytes: &[u8]) -> String {
    use vibepdf_lib::pdf::actor::DocumentActorHandle;
    let p = std::env::temp_dir().join(format!("vibepdf-del-{}.pdf", uuid::Uuid::new_v4()));
    std::fs::write(&p, bytes).expect("write temp");
    let handle = DocumentActorHandle::spawn(None, uuid::Uuid::new_v4(), p, None).expect("spawn");
    let runs = handle.read_text_runs(0).await.expect("read runs");
    drop(handle);
    runs.iter().map(|r| r.text.as_str()).collect()
}

/// A one-page PDF with two separate Tj runs, so deleting run 0 must leave run 1
/// intact — the ordinal-correctness check.
fn two_run_pdf() -> Vec<u8> {
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
            Operation::new("Tj", vec![Object::string_literal("FIRST")]),
            Operation::new("Td", vec![0.into(), (-40).into()]),
            Operation::new("Tj", vec![Object::string_literal("SECOND")]),
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
async fn delete_removes_run_from_hello() {
    let original = std::fs::read(fixture("hello.pdf")).expect("read hello.pdf");
    assert!(page0_text(&original).await.contains("VibePDF"), "fixture sanity");

    let edited = delete_text_run(&original, 0, 0).expect("delete");

    // SPEC: P6-SEC-010(c) — verify by extraction that the text is truly gone.
    let after = page0_text(&edited).await;
    assert!(!after.contains("VibePDF"), "deleted text must not re-extract: {after:?}");
}

#[tokio::test]
async fn delete_preserves_other_runs() {
    let original = two_run_pdf();
    assert_eq!(page0_text(&original).await, "FIRSTSECOND", "fixture sanity");

    // Delete run 0 ("FIRST") — run 1 ("SECOND") must survive unchanged. This is the
    // ordinal-correctness guarantee (PDFium object order == show-operator order).
    let edited = delete_text_run(&original, 0, 0).expect("delete");
    let after = page0_text(&edited).await;
    assert_eq!(after, "SECOND", "only the targeted run is removed: {after:?}");
}

#[tokio::test]
async fn delete_second_run_keeps_first() {
    let original = two_run_pdf();
    let edited = delete_text_run(&original, 0, 1).expect("delete");
    assert_eq!(page0_text(&edited).await, "FIRST", "deleting run 1 keeps run 0");
}

/// A page whose only text lives inside a Form XObject (`Do`) — there's no show
/// operator in the page content stream to splice. Deletion must **fail cleanly**,
/// never corrupt: the safety net for "PDFium sees a run the page stream doesn't."
fn xobject_text_pdf() -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let form_content = Content {
        operations: vec![
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["F1".into(), 24.into()]),
            Operation::new("Td", vec![72.into(), 700.into()]),
            Operation::new("Tj", vec![Object::string_literal("HIDDEN")]),
            Operation::new("ET", vec![]),
        ],
    }
    .encode()
    .expect("encode form");
    let form_id = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject", "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
        },
        form_content,
    ));
    let page_content = Content { operations: vec![Operation::new("Do", vec!["Fm".into()])] }
        .encode()
        .expect("encode page");
    let content_id = doc.add_object(Stream::new(dictionary! {}, page_content));
    let resources_id = doc.add_object(dictionary! { "XObject" => dictionary! { "Fm" => form_id } });
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
async fn delete_of_xobject_text_fails_safely() {
    // SPEC: P6-SEC-010 — never silently mis-delete. Whether PDFium surfaces the
    // form's text as a run or not, deletion at this index must error, not corrupt.
    let original = xobject_text_pdf();
    assert!(delete_text_run(&original, 0, 0).is_err(), "XObject-embedded text is not deletable here");
}

#[tokio::test]
async fn delete_out_of_range_errors() {
    let original = std::fs::read(fixture("hello.pdf")).expect("read hello.pdf");
    assert!(delete_text_run(&original, 0, 99).is_err(), "out-of-range run rejected");
    assert!(delete_text_run(&original, 99, 0).is_err(), "out-of-range page rejected");
}

/// Writes a deleted-run hello.pdf to /tmp for the manual three-reader + extraction
/// check. Ignored by default (produces an artifact).
#[tokio::test]
#[ignore = "produces a verification artifact; run on demand"]
async fn writes_verification_artifact() {
    let original = std::fs::read(fixture("hello.pdf")).expect("read hello.pdf");
    let edited = delete_text_run(&original, 0, 0).expect("delete");
    std::fs::write("/tmp/vibepdf-verify.pdf", &edited).expect("write artifact");
}
