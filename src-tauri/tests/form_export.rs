//! Integration tests for form-data export (P5.C1).
//!
//! SPEC: P5-FORM-008 — export name, value, and type as FDF / XFDF / JSON / CSV.

use lopdf::{Document, Object};

use vibepdf_lib::error::CommandError;
use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::form::{add_field, add_text_field, set_choice_field, NewFieldKind};
use vibepdf_lib::pdf::form_data::{
    collect_form_data, serialize, to_csv, to_fdf, to_json, to_xfdf, ExportFormat,
};

fn fixture(name: &str) -> std::path::PathBuf {
    let p = std::path::PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).expect("read fixture")
}

fn opts(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_owned()).collect()
}

/// A doc with one field of every value-carrying kind, plus a push-button.
fn mixed_form() -> Vec<u8> {
    let out = add_text_field(&read("hello.pdf"), 0, [72.0, 700.0, 260.0, 724.0], "name", "Ada", None, false, false)
        .expect("text");
    let out = add_field(&out, 0, [72.0, 660.0, 90.0, 678.0], "agree", &NewFieldKind::Checkbox { required: false })
        .expect("checkbox");
    let out = add_field(&out, 0, [72.0, 560.0, 260.0, 640.0], "color", &NewFieldKind::RadioGroup { options: opts(&["Red", "Green"]) })
        .expect("radio");
    let out = add_field(&out, 0, [72.0, 520.0, 260.0, 544.0], "fruit", &NewFieldKind::Combo { options: opts(&["Apple", "Banana"]), default: "Apple".to_owned() })
        .expect("combo");
    let out = add_field(&out, 0, [72.0, 430.0, 260.0, 500.0], "tags", &NewFieldKind::ListBox { options: opts(&["x", "y"]), multi: true })
        .expect("list");
    let out = add_field(&out, 0, [300.0, 700.0, 420.0, 724.0], "submit", &NewFieldKind::PushButton { caption: "Go".to_owned() })
        .expect("button");
    set_choice_field(&out, "tags", &["x".to_owned(), "y".to_owned()]).expect("select tags")
}

fn data_of(bytes: &[u8]) -> Vec<vibepdf_lib::pdf::form_data::FormDatum> {
    let doc = Document::load_mem(bytes).expect("load");
    collect_form_data(&doc).expect("collect")
}

#[test]
fn collects_all_value_kinds_and_excludes_pushbutton() {
    let data = data_of(&mixed_form());
    let names: Vec<&str> = data.iter().map(|d| d.name.as_str()).collect();
    assert!(names.contains(&"name") && names.contains(&"agree") && names.contains(&"color"), "{names:?}");
    assert!(names.contains(&"fruit") && names.contains(&"tags"), "{names:?}");
    assert!(!names.contains(&"submit"), "push-button excluded: {names:?}");

    let by = |n: &str| data.iter().find(|d| d.name == n).expect(n).clone();
    assert_eq!(by("name").kind, "text");
    assert_eq!(by("name").value, vec!["Ada".to_owned()]);
    assert_eq!(by("agree").kind, "checkbox");
    assert_eq!(by("color").kind, "radio");
    assert_eq!(by("fruit").value, vec!["Apple".to_owned()]);
    assert_eq!(by("tags").value, vec!["x".to_owned(), "y".to_owned()], "multi-select keeps both");
}

#[test]
fn signature_exports_with_no_value() {
    let out = add_field(&read("hello.pdf"), 0, [72.0, 700.0, 260.0, 724.0], "sig", &NewFieldKind::Signature)
        .expect("sig");
    let data = data_of(&out);
    assert_eq!(data.len(), 1);
    assert_eq!(data[0].kind, "signature");
    assert!(data[0].value.is_empty());
}

#[test]
fn fdf_is_loadable_pdf_syntax() {
    let bytes = to_fdf(&data_of(&mixed_form())).expect("fdf");
    assert!(bytes.starts_with(b"%FDF-"), "FDF header");
    // lopdf's loader insists on a `%PDF-` header, so swap it back to check the
    // *body* is valid PDF syntax (an FDF file is PDF syntax under an FDF header).
    let mut as_pdf = b"%PDF-1.2".to_vec();
    as_pdf.extend_from_slice(&bytes[b"%FDF-1.2".len()..]);
    let doc = Document::load_mem(&as_pdf).expect("FDF body parses as PDF syntax");
    let root = doc.trailer.get(b"Root").and_then(Object::as_reference).expect("root");
    let fdf = doc.get_dictionary(root).expect("catalog").get(b"FDF").expect("/FDF");
    let fdf_dict = match fdf.as_reference() {
        Ok(id) => doc.get_dictionary(id).expect("fdf dict"),
        Err(_) => fdf.as_dict().expect("fdf dict"),
    };
    let fields = fdf_dict.get(b"Fields").and_then(Object::as_array).expect("/Fields");
    assert_eq!(fields.len(), 5, "one entry per value-carrying field");
}

#[test]
fn xfdf_escapes_and_repeats_values() {
    let out = add_text_field(&read("hello.pdf"), 0, [72.0, 700.0, 260.0, 724.0], "note", "a & b < c", None, false, false)
        .expect("text");
    let x = to_xfdf(&data_of(&out));
    assert!(x.contains("a &amp; b &lt; c"), "escaped: {x}");
    assert!(x.contains("name=\"note\""));

    let multi = to_xfdf(&data_of(&mixed_form()));
    let tags_block = multi.split("name=\"tags\"").nth(1).expect("tags field");
    assert_eq!(tags_block.split("</field>").next().unwrap().matches("<value>").count(), 2);
}

#[test]
fn json_round_trips() {
    let j = to_json(&data_of(&mixed_form())).expect("json");
    let parsed: serde_json::Value = serde_json::from_str(&j).expect("valid json");
    let arr = parsed.as_array().expect("array");
    assert_eq!(arr.len(), 5);
    let first = arr.iter().find(|e| e["name"] == "name").expect("name field");
    assert_eq!(first["type"], "text");
    assert_eq!(first["value"][0], "Ada");
}

#[test]
fn csv_has_header_and_joins_multi_values() {
    let c = to_csv(&data_of(&mixed_form()));
    assert!(c.starts_with("name,type,value\n"), "{c}");
    assert!(c.contains("tags,list,x;y"), "{c}");
}

#[test]
fn unicode_value_survives() {
    let out = add_text_field(&read("hello.pdf"), 0, [72.0, 700.0, 260.0, 724.0], "who", "Ådàm café", None, false, false)
        .expect("text");
    assert_eq!(data_of(&out)[0].value, vec!["Ådàm café".to_owned()]);
    assert!(to_json(&data_of(&out)).expect("json").contains("Ådàm café"));
}

#[test]
fn no_form_exports_nothing() {
    assert!(data_of(&read("hello.pdf")).is_empty());
}

#[test]
fn unknown_format_rejected() {
    let err = ExportFormat::parse("xlsx").unwrap_err();
    assert!(matches!(err, CommandError::InvalidInput(_)), "got {err:?}");
}

#[tokio::test]
async fn exports_through_the_actor() {
    let src = std::env::temp_dir().join(format!("vibepdf-c1-{}.pdf", uuid::Uuid::new_v4()));
    std::fs::write(&src, mixed_form()).expect("write");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, src, None).expect("spawn");

    let dest = std::env::temp_dir().join(format!("vibepdf-c1-{}.json", uuid::Uuid::new_v4()));
    let count = handle.export_form_data(ExportFormat::Json, dest.clone()).await.expect("export");
    assert_eq!(count, 5);
    let written = std::fs::read_to_string(&dest).expect("read export");
    assert!(written.contains("\"name\": \"name\""), "{written}");
    drop(handle);
}

#[test]
#[ignore = "produces verification artifacts; run on demand"]
fn writes_verification_artifacts() {
    let data = data_of(&mixed_form());
    for (fmt, ext) in [
        (ExportFormat::Fdf, "fdf"),
        (ExportFormat::Xfdf, "xfdf"),
        (ExportFormat::Json, "json"),
        (ExportFormat::Csv, "csv"),
    ] {
        let bytes = serialize(&data, fmt).expect("serialize");
        let path = std::env::temp_dir().join(format!("vibepdf-verify-formdata.{ext}"));
        std::fs::write(&path, bytes).expect("write");
        eprintln!("wrote {}", path.display());
    }
}
