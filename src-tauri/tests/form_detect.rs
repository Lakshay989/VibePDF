//! Integration tests for AcroForm detection (P5.A1).
//!
//! SPEC: P5-FORM-001 — "WHEN the user opens a PDF containing AcroForm fields,
//! THE system SHALL detect them and display a 'Form mode' entry point with field
//! count." These exercise the real read path: the byte-level reader and the
//! document actor's `read_form_summary`, against the `forms.pdf` fixture (one
//! terminal text field) and an in-test no-form document. The field-tree walk
//! semantics (radio groups, hierarchy, XFA) are unit-tested in `pdf::form`.

use std::path::PathBuf;

use lopdf::{dictionary, Document, Object};

use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::form::read_form_summary;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

#[test]
fn forms_pdf_reports_one_field_via_bytes() {
    let bytes = std::fs::read(fixture("forms.pdf")).expect("read forms.pdf");
    let summary = read_form_summary(&bytes).expect("summary");
    assert_eq!(summary.field_count, 1, "forms.pdf has one text field");
    assert!(!summary.has_xfa);
}

#[tokio::test]
async fn forms_pdf_reports_one_field_via_actor() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("forms.pdf"), None).expect("spawn");
    let summary = handle.read_form_summary().await.expect("read summary");
    assert_eq!(summary.field_count, 1);
    assert!(!summary.has_xfa);
    drop(handle);
}

#[test]
fn no_acroform_reports_zero() {
    // A minimal one-page PDF with no `/AcroForm` at all.
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(
            dictionary! { "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1 },
        ),
    );
    let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog_id);

    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("serialize");

    let summary = read_form_summary(&bytes).expect("summary");
    assert_eq!(summary.field_count, 0);
    assert!(!summary.has_xfa);
}
