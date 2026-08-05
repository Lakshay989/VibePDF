//! Integration tests for filling choice fields (combo / list) — P5.A4.
//!
//! SPEC: P5-FORM-004 — display a field's `/Opt` options and select per its
//! single/multi flag. These verify the read (options incl. export/display pairs,
//! kind, multi, current selection) and the write (`/V` as a string or array, `/I`
//! indices for list boxes, `/NeedAppearances`, and rejection of undeclared values).

use lopdf::{Document, Object};

use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::error::CommandError;
use vibepdf_lib::pdf::form::{read_choice_fields, set_choice_field};

fn fixture(name: &str) -> std::path::PathBuf {
    let p = std::path::PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).expect("read fixture")
}

#[test]
fn reads_combo_and_list() {
    let f = read_choice_fields(&read("forms-choice.pdf"), 0).expect("read");
    assert_eq!(f.len(), 2);
    let combo = f.iter().find(|x| x.name == "fruit").expect("combo");
    assert_eq!(combo.kind, "combo");
    assert!(!combo.multi);
    assert_eq!(combo.options.len(), 3);
    assert_eq!(combo.selected, vec!["Apple".to_owned()]);
    let list = f.iter().find(|x| x.name == "colors").expect("list");
    assert_eq!(list.kind, "list");
    assert!(list.multi);
    assert!(list.selected.is_empty());
}

#[test]
fn labeled_option_export_vs_display() {
    let combo = read_choice_fields(&read("forms-choice.pdf"), 0)
        .expect("read")
        .into_iter()
        .find(|x| x.name == "fruit")
        .unwrap();
    let cherry = combo.options.iter().find(|o| o.label == "Cherry").expect("cherry");
    assert_eq!(cherry.export, "chy", "export differs from display");
}

#[test]
fn selects_a_combo_value() {
    let out = set_choice_field(&read("forms-choice.pdf"), "fruit", &["Banana".to_owned()]).expect("set");
    let combo = read_choice_fields(&out, 0).expect("read").into_iter().find(|x| x.name == "fruit").unwrap();
    assert_eq!(combo.selected, vec!["Banana".to_owned()]);
    assert!(!v_is_array(&out, "fruit"), "single selection stored as a string");
}

#[test]
fn multi_select_list() {
    let out = set_choice_field(&read("forms-choice.pdf"), "colors", &["Red".to_owned(), "Blue".to_owned()])
        .expect("set");
    let list = read_choice_fields(&out, 0).expect("read").into_iter().find(|x| x.name == "colors").unwrap();
    assert_eq!(list.selected, vec!["Red".to_owned(), "Blue".to_owned()]);
    assert!(v_is_array(&out, "colors"), "multi selection stored as an array");
    assert_eq!(indices(&out, "colors"), vec![0, 2], "/I indices ascending");
}

#[test]
fn rejects_unknown_value() {
    let err = set_choice_field(&read("forms-choice.pdf"), "fruit", &["Mango".to_owned()]).unwrap_err();
    assert!(matches!(err, CommandError::InvalidInput(_)), "got {err:?}");
}

#[test]
fn sets_need_appearances() {
    assert!(!has_need_appearances(&read("forms-choice.pdf")), "fixture starts without it");
    let out = set_choice_field(&read("forms-choice.pdf"), "fruit", &["Banana".to_owned()]).expect("set");
    assert!(has_need_appearances(&out));
}

#[tokio::test]
async fn set_then_undo() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("forms-choice.pdf"), None).expect("spawn");
    handle.set_choice_field("fruit".to_owned(), vec!["Banana".to_owned()]).await.expect("set");
    let after = handle.read_choice_fields(0).await.expect("read");
    assert_eq!(after.iter().find(|x| x.name == "fruit").unwrap().selected, vec!["Banana".to_owned()]);
    handle.undo().await.expect("undo");
    let reverted = handle.read_choice_fields(0).await.expect("read");
    assert_eq!(reverted.iter().find(|x| x.name == "fruit").unwrap().selected, vec!["Apple".to_owned()]);
    drop(handle);
}

#[test]
#[ignore = "produces a verification artifact; run on demand"]
fn writes_verification_artifact() {
    let out = set_choice_field(&read("forms-choice.pdf"), "fruit", &["chy".to_owned()]).expect("combo");
    let out = set_choice_field(&out, "colors", &["Green".to_owned(), "Blue".to_owned()]).expect("list");
    let path = std::env::temp_dir().join("vibepdf-verify.pdf");
    std::fs::write(&path, &out).expect("write artifact");
    eprintln!("wrote {}", path.display());
}

// ── test helpers ────────────────────────────────────────────────────────────

fn field_by_t(doc: &Document, name: &str) -> Option<lopdf::ObjectId> {
    let root = doc.trailer.get(b"Root").and_then(Object::as_reference).ok()?;
    let acro = doc.get_dictionary(root).ok()?.get(b"AcroForm").ok()?;
    let acro_d = match acro.as_reference() {
        Ok(id) => doc.get_dictionary(id).ok()?,
        Err(_) => acro.as_dict().ok()?,
    };
    let fields = acro_d.get(b"Fields").ok()?.as_array().ok()?;
    for f in fields {
        if let Ok(id) = f.as_reference() {
            if let Ok(d) = doc.get_dictionary(id) {
                if let Ok(t) = d.get(b"T").and_then(Object::as_str) {
                    if String::from_utf8_lossy(t) == name {
                        return Some(id);
                    }
                }
            }
        }
    }
    None
}

fn v_is_array(bytes: &[u8], name: &str) -> bool {
    let doc = Document::load_mem(bytes).expect("load");
    let id = field_by_t(&doc, name).expect("field");
    matches!(doc.get_dictionary(id).and_then(|d| d.get(b"V")), Ok(Object::Array(_)))
}

fn indices(bytes: &[u8], name: &str) -> Vec<i64> {
    let doc = Document::load_mem(bytes).expect("load");
    let id = field_by_t(&doc, name).expect("field");
    doc.get_dictionary(id)
        .ok()
        .and_then(|d| d.get(b"I").ok())
        .and_then(|o| o.as_array().ok())
        .map(|a| a.iter().filter_map(|x| x.as_i64().ok()).collect())
        .unwrap_or_default()
}

fn has_need_appearances(bytes: &[u8]) -> bool {
    let doc = Document::load_mem(bytes).expect("load");
    let Ok(root) = doc.trailer.get(b"Root").and_then(Object::as_reference) else { return false };
    let Ok(acro) = doc.get_dictionary(root).and_then(|c| c.get(b"AcroForm")) else { return false };
    let acro_d = match acro.as_reference() {
        Ok(id) => doc.get_dictionary(id).ok(),
        Err(_) => acro.as_dict().ok(),
    };
    acro_d.is_some_and(|a| matches!(a.get(b"NeedAppearances"), Ok(Object::Boolean(true))))
}
