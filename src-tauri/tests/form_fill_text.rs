//! Integration tests for filling AcroForm text fields (P5.A2).
//!
//! SPEC: P5-FORM-002 — "WHEN the user clicks an AcroForm text field, THE system
//! SHALL allow typing, support tab navigation, and respect maximum-length
//! constraints." These exercise the read (geometry + flags) and the write (`/V` +
//! `/NeedAppearances`, truncated to `/MaxLen`), verifying by re-reading the value.
//! Tab navigation is a frontend concern (native DOM order) and is covered there.

use lopdf::{dictionary, Document, Object};

use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::form::{read_text_fields, set_text_field_value};

fn fixture(name: &str) -> std::path::PathBuf {
    let p = std::path::PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).expect("read fixture")
}

#[test]
fn reads_single_field_geometry() {
    let fields = read_text_fields(&read("forms.pdf"), 0).expect("read fields");
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].name, "name");
    assert!(!fields[0].multiline);
    assert_eq!(fields[0].max_len, None);
    let r = fields[0].rect;
    assert!((r[0] - 72.0).abs() < 0.5 && (r[3] - 724.0).abs() < 0.5, "rect {r:?}");
}

#[test]
fn reads_multi_field_flags() {
    let fields = read_text_fields(&read("forms-multi.pdf"), 0).expect("read fields");
    assert_eq!(fields.len(), 3);
    let last = fields.iter().find(|f| f.name == "last").expect("last field");
    assert_eq!(last.max_len, Some(5));
    let notes = fields.iter().find(|f| f.name == "notes").expect("notes field");
    assert!(notes.multiline);
}

#[test]
fn fills_value_and_reads_back() {
    let out = set_text_field_value(&read("forms.pdf"), "name", "Ada Lovelace").expect("fill");
    let fields = read_text_fields(&out, 0).expect("read fields");
    assert_eq!(fields[0].value, "Ada Lovelace");
}

#[test]
fn respects_max_len() {
    let out = set_text_field_value(&read("forms-multi.pdf"), "last", "abcdefgh").expect("fill");
    let last = read_text_fields(&out, 0)
        .expect("read fields")
        .into_iter()
        .find(|f| f.name == "last")
        .expect("last field");
    assert_eq!(last.value, "abcde", "value truncated to /MaxLen 5");
}

#[test]
fn unicode_value_round_trips() {
    let out = set_text_field_value(&read("forms.pdf"), "name", "Ådàm café").expect("fill");
    let fields = read_text_fields(&out, 0).expect("read fields");
    assert_eq!(fields[0].value, "Ådàm café");
}

#[test]
fn fill_sets_need_appearances() {
    // A form WITHOUT /NeedAppearances → the fill must set it so viewers regenerate.
    let mut doc = Document::with_version("1.5");
    let field = doc.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "FT" => "Tx",
        "T" => Object::string_literal("x"),
        "Rect" => vec![10.into(), 10.into(), 100.into(), 30.into()],
    });
    let acro = doc.add_object(dictionary! { "Fields" => vec![field.into()] });
    let page_tree = doc.new_object_id();
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => page_tree,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Annots" => vec![field.into()],
    });
    doc.objects.insert(
        page_tree,
        Object::Dictionary(
            dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 },
        ),
    );
    let catalog = doc.add_object(dictionary! {
        "Type" => "Catalog", "Pages" => page_tree, "AcroForm" => acro,
    });
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("serialize");

    let out = set_text_field_value(&bytes, "x", "hi").expect("fill");
    assert!(need_appearances(&out), "/NeedAppearances set after fill");
}

/// Read the AcroForm's `/NeedAppearances` from a byte buffer.
fn need_appearances(bytes: &[u8]) -> bool {
    let doc = Document::load_mem(bytes).expect("load");
    let root = doc.trailer.get(b"Root").and_then(Object::as_reference).expect("root");
    let acro = doc.get_dictionary(root).expect("catalog").get(b"AcroForm").expect("acroform");
    let acro_dict = match acro.as_reference() {
        Ok(id) => doc.get_dictionary(id).expect("acro dict"),
        Err(_) => acro.as_dict().expect("acro dict"),
    };
    matches!(acro_dict.get(b"NeedAppearances"), Ok(Object::Boolean(true)))
}

#[tokio::test]
async fn fill_via_actor_then_undo() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("forms.pdf"), None).expect("spawn");

    handle.fill_text_field("name".to_owned(), "Grace".to_owned()).await.expect("fill");
    let filled = handle.read_text_fields(0).await.expect("read");
    assert_eq!(filled[0].value, "Grace");

    handle.undo().await.expect("undo");
    let reverted = handle.read_text_fields(0).await.expect("read");
    assert_eq!(reverted[0].value, "", "undo clears the value");
    drop(handle);
}

/// Produce a filled form for a manual cross-reader check (a mainstream reader/Preview should
/// show the values and keep the form interactive). Run on demand.
#[test]
#[ignore = "produces a verification artifact; run on demand"]
fn writes_verification_artifact() {
    let out = set_text_field_value(&read("forms-multi.pdf"), "first", "Ada Lovelace").expect("first");
    let out = set_text_field_value(&out, "last", "Byron").expect("last");
    let out = set_text_field_value(&out, "notes", "Filled by VibePDF").expect("notes");
    let path = std::env::temp_dir().join("vibepdf-verify.pdf");
    std::fs::write(&path, &out).expect("write artifact");
    eprintln!("wrote {}", path.display());
}
