//! Integration tests for the font-fallback resolver (P4.A2).
//!
//! SPEC: P4-EDIT-002 — a non-embedded, non-installed font must resolve to a
//! base-14 substitute and raise the once-per-document warning; an embedded or
//! base-14 font must not. Exercised through the actor's `read_font_report`,
//! which walks the *live* PDFium document (the same read path as A1).

use std::path::PathBuf;

use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Document, Object, Stream};

use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::font_resolver::FontStatus;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn spawn_path(path: PathBuf) -> DocumentActorHandle {
    let id = uuid::Uuid::new_v4();
    DocumentActorHandle::spawn(None, id, path, None).expect("spawn")
}

/// Build a one-page PDF that shows text in `/BaseFont Calibri` with **no**
/// `FontFile` — i.e. a font that's neither embedded nor a base-14 face. PDFium
/// still creates the text object and reports the BaseFont name + embedded=false,
/// which is exactly the case A2 must flag. Returns a temp path.
fn write_non_embedded_calibri_pdf() -> PathBuf {
    let mut doc = Document::with_version("1.5");

    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Calibri",
    });
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });

    let content = Content {
        operations: vec![
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["F1".into(), 24.into()]),
            Operation::new("Td", vec![72.into(), 700.into()]),
            Operation::new("Tj", vec![Object::string_literal("Hello Calibri")]),
            Operation::new("ET", vec![]),
        ],
    };
    let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().expect("encode")));

    let pages_id = doc.new_object_id();
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "Contents" => content_id,
        "Resources" => resources_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);

    let path = std::env::temp_dir().join(format!("vibepdf-fontfallback-{}.pdf", uuid::Uuid::new_v4()));
    doc.save(&path).expect("save calibri pdf");
    path
}

#[tokio::test]
async fn non_embedded_custom_font_needs_fallback() {
    let handle = spawn_path(write_non_embedded_calibri_pdf());
    let report = handle.read_font_report().await.expect("font report");

    assert!(report.needs_fallback, "non-embedded Calibri → fallback");
    let calibri = report
        .fonts
        .iter()
        .find(|f| f.font_name.contains("Calibri"))
        .expect("Calibri reported");
    assert!(!calibri.embedded, "Calibri is not embedded");
    assert_eq!(calibri.status, FontStatus::Fallback);
    // Sans-serif unknown → the Helvetica representative of the fallback stack.
    assert_eq!(calibri.substitute.as_deref(), Some("Helvetica"));

    drop(handle);
}

#[tokio::test]
async fn base_14_font_needs_no_fallback() {
    // hello.pdf shows text in Helvetica — base-14, so safe to edit anywhere.
    let handle = spawn_path(fixture("hello.pdf"));
    let report = handle.read_font_report().await.expect("font report");

    assert!(!report.needs_fallback, "Helvetica is base-14: {:?}", report.fonts);
    assert!(
        report.fonts.iter().all(|f| f.status != FontStatus::Fallback),
        "no fallback entries: {:?}",
        report.fonts
    );
    // Every fallback entry (none here) would carry a substitute; non-fallback
    // entries never do.
    for f in &report.fonts {
        assert!(f.substitute.is_none(), "{} is safe → no substitute", f.font_name);
    }
    drop(handle);
}

#[tokio::test]
async fn report_is_well_formed_across_documents() {
    for name in ["hello.pdf", "links.pdf", "forms.pdf"] {
        let handle = spawn_path(fixture(name));
        let report = handle.read_font_report().await.expect("font report");
        // Invariants: needs_fallback iff some entry is Fallback; a substitute is
        // present exactly when the status is Fallback.
        let any_fallback = report.fonts.iter().any(|f| f.status == FontStatus::Fallback);
        assert_eq!(report.needs_fallback, any_fallback, "{name}");
        for f in &report.fonts {
            assert_eq!(
                f.substitute.is_some(),
                f.status == FontStatus::Fallback,
                "{name}: {} substitute matches status",
                f.font_name
            );
        }
        drop(handle);
    }
}
