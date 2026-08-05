//! Integration tests for creating text fields (P5.B1).
//!
//! SPEC: P5-FORM-006 — create a text field configured with name, default value,
//! max length, multi-line, and required flag. These verify the field is created
//! (readable via A2's `read_text_fields`), an AcroForm is created when absent,
//! the flags/default land, duplicate names are rejected, and undo removes it.

use lopdf::{Document, Object};

use vibepdf_lib::error::CommandError;
use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::form::{add_text_field, read_form_summary, read_text_fields};

fn fixture(name: &str) -> std::path::PathBuf {
    let p = std::path::PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).expect("read fixture")
}

const RECT: [f32; 4] = [72.0, 700.0, 300.0, 724.0];

#[test]
fn creates_a_text_field() {
    // hello.pdf has no form; adding a field creates the AcroForm + widget.
    let out = add_text_field(&read("hello.pdf"), 0, RECT, "email", "", Some(64), false, false)
        .expect("add");
    let fields = read_text_fields(&out, 0).expect("read");
    let f = fields.iter().find(|x| x.name == "email").expect("field");
    assert_eq!(f.max_len, Some(64));
    assert!(!f.multiline);
    assert!((f.rect[0] - 72.0).abs() < 0.5 && (f.rect[3] - 724.0).abs() < 0.5, "rect {:?}", f.rect);
}

#[test]
fn creates_acroform_when_absent() {
    let out = add_text_field(&read("hello.pdf"), 0, RECT, "email", "", None, false, false).expect("add");
    assert_eq!(read_form_summary(&out).expect("summary").field_count, 1);
}

#[test]
fn default_value_and_flags() {
    let out = add_text_field(&read("hello.pdf"), 0, RECT, "notes", "hello", None, true, true).expect("add");
    let f = read_text_fields(&out, 0).expect("read").into_iter().find(|x| x.name == "notes").unwrap();
    assert_eq!(f.value, "hello", "default in /V");
    assert!(f.multiline, "multiline flag");
    // Required is /Ff bit 2 (1<<1); multiline is bit 13 (1<<12).
    assert_eq!(field_ff(&out, "notes") & ((1 << 1) | (1 << 12)), (1 << 1) | (1 << 12));
}

#[test]
fn rejects_duplicate_name() {
    // forms.pdf already has a field named "name".
    let err = add_text_field(&read("forms.pdf"), 0, RECT, "name", "", None, false, false).unwrap_err();
    assert!(matches!(err, CommandError::InvalidInput(_)), "got {err:?}");
}

#[test]
fn add_into_existing_acroform() {
    let out = add_text_field(&read("forms.pdf"), 0, [72.0, 640.0, 300.0, 664.0], "email", "", None, false, false)
        .expect("add");
    let names: Vec<String> = read_text_fields(&out, 0).expect("read").into_iter().map(|f| f.name).collect();
    assert!(names.contains(&"name".to_owned()) && names.contains(&"email".to_owned()), "{names:?}");
}

#[test]
fn sets_need_appearances() {
    let out = add_text_field(&read("hello.pdf"), 0, RECT, "email", "", None, false, false).expect("add");
    assert!(has_need_appearances(&out));
}

#[test]
fn rejects_empty_name() {
    let err = add_text_field(&read("hello.pdf"), 0, RECT, "  ", "", None, false, false).unwrap_err();
    assert!(matches!(err, CommandError::InvalidInput(_)), "got {err:?}");
}

#[tokio::test]
async fn create_then_undo() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");
    handle
        .add_text_field(0, RECT, "email".to_owned(), String::new(), None, false, false)
        .await
        .expect("add");
    assert_eq!(handle.read_form_summary().await.expect("summary").field_count, 1);
    handle.undo().await.expect("undo");
    assert_eq!(handle.read_form_summary().await.expect("summary").field_count, 0, "undo removes it");
    drop(handle);
}

#[test]
#[ignore = "produces a verification artifact; run on demand"]
fn writes_verification_artifact() {
    let out = add_text_field(&read("hello.pdf"), 0, RECT, "email", "you@example.com", Some(64), false, true)
        .expect("add");
    let path = std::env::temp_dir().join("vibepdf-verify.pdf");
    std::fs::write(&path, &out).expect("write artifact");
    eprintln!("wrote {}", path.display());
}

fn field_ff(bytes: &[u8], name: &str) -> i64 {
    let doc = Document::load_mem(bytes).expect("load");
    for (id, obj) in &doc.objects {
        if let Object::Dictionary(d) = obj {
            if d.get(b"T").and_then(Object::as_str).ok().map(|t| String::from_utf8_lossy(t)) == Some(name.into())
            {
                let _ = id;
                return d.get(b"Ff").and_then(Object::as_i64).unwrap_or(0);
            }
        }
    }
    0
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
