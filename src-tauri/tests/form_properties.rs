//! Integration tests for field properties, tab order, and delete (P5.B3).
//!
//! SPEC: P5-FORM-006b / -006c (drafted; see steps/P5.md B3) — edit an existing
//! field's properties, reorder the page's tab sequence, and delete a field.

use lopdf::{Document, Object};

use vibepdf_lib::error::CommandError;
use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::form::{
    add_field, add_text_field, delete_field, read_form_summary, read_page_fields, read_text_fields,
    set_tab_order, update_field_properties, FieldProperties, NewFieldKind,
};

fn fixture(name: &str) -> std::path::PathBuf {
    let p = std::path::PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).expect("read fixture")
}

/// A doc with three fields on page 0: text "a", checkbox "b", text "c".
fn three_fields() -> Vec<u8> {
    let out = add_text_field(&read("hello.pdf"), 0, [72.0, 700.0, 200.0, 724.0], "a", "", None, false, false)
        .expect("a");
    let out = add_field(&out, 0, [72.0, 660.0, 90.0, 678.0], "b", &NewFieldKind::Checkbox { required: false })
        .expect("b");
    add_text_field(&out, 0, [72.0, 620.0, 200.0, 644.0], "c", "", None, false, false).expect("c")
}

#[test]
fn reads_page_fields_in_order() {
    let f = read_page_fields(&three_fields(), 0).expect("read");
    let names: Vec<&str> = f.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(names, vec!["a", "b", "c"]);
    assert_eq!(f[1].kind, "checkbox");
    assert_eq!(f[0].kind, "text");
}

#[test]
fn renames_a_field() {
    let out = update_field_properties(&three_fields(), "a", &FieldProperties {
        new_name: Some("first".to_owned()),
        ..FieldProperties::default()
    })
    .expect("rename");
    let names: Vec<String> = read_text_fields(&out, 0).expect("read").into_iter().map(|f| f.name).collect();
    assert!(names.contains(&"first".to_owned()) && !names.contains(&"a".to_owned()), "{names:?}");
}

#[test]
fn rename_collision_rejected() {
    let err = update_field_properties(&three_fields(), "a", &FieldProperties {
        new_name: Some("c".to_owned()),
        ..FieldProperties::default()
    })
    .unwrap_err();
    assert!(matches!(err, CommandError::InvalidInput(_)), "got {err:?}");
}

#[test]
fn edits_value_maxlen_flags_tooltip() {
    let out = update_field_properties(&three_fields(), "a", &FieldProperties {
        default_value: Some("hi".to_owned()),
        max_len: Some(Some(5)),
        multiline: Some(true),
        required: Some(true),
        tooltip: Some("Your name".to_owned()),
        ..FieldProperties::default()
    })
    .expect("update");
    let f = read_text_fields(&out, 0).expect("read").into_iter().find(|x| x.name == "a").unwrap();
    assert_eq!(f.value, "hi");
    assert_eq!(f.max_len, Some(5));
    assert!(f.multiline);
    assert_eq!(entry_str(&out, "a", b"TU").as_deref(), Some("Your name"));
    assert_ne!(entry_i64(&out, "a", b"Ff") & (1 << 1), 0, "required bit");
}

#[test]
fn clears_maxlen_and_tooltip() {
    let out = update_field_properties(&three_fields(), "a", &FieldProperties {
        max_len: Some(Some(9)),
        tooltip: Some("t".to_owned()),
        ..FieldProperties::default()
    })
    .expect("set");
    let out = update_field_properties(&out, "a", &FieldProperties {
        max_len: Some(None),
        tooltip: Some(String::new()),
        ..FieldProperties::default()
    })
    .expect("clear");
    let f = read_text_fields(&out, 0).expect("read").into_iter().find(|x| x.name == "a").unwrap();
    assert_eq!(f.max_len, None);
    assert_eq!(entry_str(&out, "a", b"TU"), None);
}

#[test]
fn unknown_field_errors() {
    let err = update_field_properties(&three_fields(), "nope", &FieldProperties::default()).unwrap_err();
    assert!(matches!(err, CommandError::NotFound(_)), "got {err:?}");
}

#[test]
fn sets_tab_order_and_tabs_key() {
    let out = set_tab_order(&three_fields(), 0, &["c".to_owned(), "a".to_owned(), "b".to_owned()])
        .expect("order");
    let names: Vec<String> = read_page_fields(&out, 0).expect("read").into_iter().map(|f| f.name).collect();
    assert_eq!(names, vec!["c".to_owned(), "a".to_owned(), "b".to_owned()]);
    assert_eq!(page_tabs(&out), Some("S".to_owned()), "/Tabs /S set");
}

#[test]
fn tab_order_keeps_unlisted_fields() {
    let out = set_tab_order(&three_fields(), 0, &["c".to_owned()]).expect("order");
    let names: Vec<String> = read_page_fields(&out, 0).expect("read").into_iter().map(|f| f.name).collect();
    assert_eq!(names, vec!["c".to_owned(), "a".to_owned(), "b".to_owned()], "listed first, rest in order");
}

#[test]
fn deletes_a_field() {
    let out = delete_field(&three_fields(), "b").expect("delete");
    let names: Vec<String> = read_page_fields(&out, 0).expect("read").into_iter().map(|f| f.name).collect();
    assert_eq!(names, vec!["a".to_owned(), "c".to_owned()]);
    assert_eq!(read_form_summary(&out).expect("summary").field_count, 2);
}

#[test]
fn deletes_a_radio_group_with_its_kids() {
    let base = add_field(&read("hello.pdf"), 0, [72.0, 600.0, 260.0, 680.0], "color", &NewFieldKind::RadioGroup {
        options: vec!["Red".to_owned(), "Green".to_owned()],
    })
    .expect("radio");
    let out = delete_field(&base, "color").expect("delete");
    assert!(read_page_fields(&out, 0).expect("read").is_empty(), "group + kids gone");
    assert_eq!(read_form_summary(&out).expect("summary").field_count, 0);
}

#[tokio::test]
async fn properties_then_undo() {
    let path = std::env::temp_dir().join(format!("vibepdf-b3-{}.pdf", uuid::Uuid::new_v4()));
    std::fs::write(&path, three_fields()).expect("write");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, path, None).expect("spawn");

    handle
        .update_field_properties("a".to_owned(), FieldProperties {
            new_name: Some("first".to_owned()),
            ..FieldProperties::default()
        })
        .await
        .expect("rename");
    let names: Vec<String> = handle.read_page_fields(0).await.expect("read").into_iter().map(|f| f.name).collect();
    assert!(names.contains(&"first".to_owned()));

    handle.undo().await.expect("undo");
    let names: Vec<String> = handle.read_page_fields(0).await.expect("read").into_iter().map(|f| f.name).collect();
    assert!(names.contains(&"a".to_owned()), "undo restored the name");
    drop(handle);
}

#[test]
#[ignore = "produces a verification artifact; run on demand"]
fn writes_verification_artifact() {
    let out = update_field_properties(&three_fields(), "a", &FieldProperties {
        new_name: Some("full_name".to_owned()),
        tooltip: Some("Your full name".to_owned()),
        required: Some(true),
        ..FieldProperties::default()
    })
    .expect("props");
    let out = set_tab_order(&out, 0, &["c".to_owned(), "full_name".to_owned()]).expect("order");
    let path = std::env::temp_dir().join("vibepdf-verify.pdf");
    std::fs::write(&path, &out).expect("write artifact");
    eprintln!("wrote {}", path.display());
}

// ── helpers ─────────────────────────────────────────────────────────────────

fn field_dict_entry(bytes: &[u8], name: &str, key: &[u8]) -> Option<Object> {
    let doc = Document::load_mem(bytes).expect("load");
    for obj in doc.objects.values() {
        if let Object::Dictionary(d) = obj {
            if d.get(b"T").and_then(Object::as_str).ok().map(|t| String::from_utf8_lossy(t)) == Some(name.into())
            {
                return d.get(key).ok().cloned();
            }
        }
    }
    None
}

fn entry_str(bytes: &[u8], name: &str, key: &[u8]) -> Option<String> {
    field_dict_entry(bytes, name, key)
        .and_then(|o| o.as_str().ok().map(|s| String::from_utf8_lossy(s).into_owned()))
}

fn entry_i64(bytes: &[u8], name: &str, key: &[u8]) -> i64 {
    field_dict_entry(bytes, name, key).and_then(|o| o.as_i64().ok()).unwrap_or(0)
}

fn page_tabs(bytes: &[u8]) -> Option<String> {
    let doc = Document::load_mem(bytes).expect("load");
    let (_, page_id) = doc.get_pages().into_iter().next()?;
    doc.get_dictionary(page_id)
        .ok()?
        .get(b"Tabs")
        .ok()?
        .as_name()
        .ok()
        .map(|n| String::from_utf8_lossy(n).into_owned())
}
