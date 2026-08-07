//! Integration tests for form-data import (P5.C2).
//!
//! SPEC: P5-FORM-009 — "WHEN the user imports form data, THE system SHALL fill
//! matching fields by name. Unmatched fields SHALL be reported. Type mismatches
//! SHALL be reported, not silently coerced."
//!
//! The round-trip tests are the load-bearing ones: they go through the real
//! export serialisers (P5.C1), so a change that breaks either half is caught.

use lopdf::Document;

use vibepdf_lib::error::CommandError;
use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::form::{
    add_field, add_text_field, set_button_field, set_choice_field, NewFieldKind,
};
use vibepdf_lib::pdf::form_data::{collect_form_data, serialize, ExportFormat, FormDatum};
use vibepdf_lib::pdf::form_import::import_form_data;

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

/// A blank form: one field of every value-carrying kind, no values set.
fn blank_form() -> Vec<u8> {
    let out = add_text_field(&read("hello.pdf"), 0, [72.0, 700.0, 260.0, 724.0], "name", "", None, false, false)
        .expect("text");
    let out = add_field(&out, 0, [72.0, 660.0, 90.0, 678.0], "agree", &NewFieldKind::Checkbox { required: false })
        .expect("checkbox");
    let out = add_field(&out, 0, [72.0, 560.0, 260.0, 640.0], "color", &NewFieldKind::RadioGroup { options: opts(&["Red", "Green"]) })
        .expect("radio");
    let out = add_field(&out, 0, [72.0, 520.0, 260.0, 544.0], "fruit", &NewFieldKind::Combo { options: opts(&["Apple", "Banana"]), default: String::new() })
        .expect("combo");
    add_field(&out, 0, [72.0, 430.0, 260.0, 500.0], "tags", &NewFieldKind::ListBox { options: opts(&["x", "y"]), multi: true })
        .expect("list")
}

/// The same form with every field filled — the source of the exported data.
fn filled_form() -> Vec<u8> {
    let out = blank_form();
    let out = vibepdf_lib::pdf::form::set_text_field_value(&out, "name", "Ada").expect("name");
    let out = set_button_field(&out, "agree", "Yes", true).expect("agree");
    let out = set_button_field(&out, "color", "Green", true).expect("color");
    let out = set_choice_field(&out, "fruit", &["Banana".to_owned()]).expect("fruit");
    set_choice_field(&out, "tags", &["x".to_owned(), "y".to_owned()]).expect("tags")
}

fn data_of(bytes: &[u8]) -> Vec<FormDatum> {
    let doc = Document::load_mem(bytes).expect("load");
    collect_form_data(&doc).expect("collect")
}

fn value_of(bytes: &[u8], name: &str) -> Vec<String> {
    data_of(bytes).into_iter().find(|d| d.name == name).map(|d| d.value).unwrap_or_default()
}

/// Export `filled_form` in `format`, import it into a blank copy, and return the
/// result — the acceptance-demo path ("export filled data → re-import to a blank
/// copy → fields restored identically").
fn round_trip(format: ExportFormat) -> Vec<u8> {
    let data = serialize(&data_of(&filled_form()), format).expect("serialize");
    let (out, report) = import_form_data(&blank_form(), &data, format).expect("import");
    assert!(report.unmatched.is_empty(), "unmatched: {:?}", report.unmatched);
    assert!(report.mismatched.is_empty(), "mismatched: {:?}", report.mismatched);
    out
}

#[test]
fn json_round_trip_restores_every_field() {
    let out = round_trip(ExportFormat::Json);
    assert_eq!(value_of(&out, "name"), vec!["Ada".to_owned()]);
    assert_eq!(value_of(&out, "agree"), vec!["Yes".to_owned()]);
    assert_eq!(value_of(&out, "color"), vec!["Green".to_owned()]);
    assert_eq!(value_of(&out, "fruit"), vec!["Banana".to_owned()]);
    assert_eq!(value_of(&out, "tags"), vec!["x".to_owned(), "y".to_owned()]);
}

#[test]
fn every_format_round_trips_the_same_values() {
    // FDF and XFDF carry no type, so they exercise the shape-only validation path.
    for fmt in [ExportFormat::Fdf, ExportFormat::Xfdf, ExportFormat::Csv] {
        let out = round_trip(fmt);
        assert_eq!(value_of(&out, "name"), vec!["Ada".to_owned()], "{fmt:?}");
        assert_eq!(value_of(&out, "fruit"), vec!["Banana".to_owned()], "{fmt:?}");
        assert_eq!(value_of(&out, "tags"), vec!["x".to_owned(), "y".to_owned()], "{fmt:?}");
        assert_eq!(value_of(&out, "agree"), vec!["Yes".to_owned()], "{fmt:?}");
    }
}

#[test]
fn unmatched_names_are_reported_and_nothing_else_breaks() {
    let json = br#"[{"name":"name","type":"text","value":["Ada"]},
                    {"name":"nope","type":"text","value":["x"]}]"#;
    let (out, report) = import_form_data(&blank_form(), json, ExportFormat::Json).expect("import");
    assert_eq!(report.applied, 1);
    assert_eq!(report.unmatched, vec!["nope".to_owned()]);
    assert_eq!(value_of(&out, "name"), vec!["Ada".to_owned()], "the matching field still filled");
}

#[test]
fn declared_type_mismatch_is_reported_and_not_coerced() {
    // The data calls `name` a checkbox; the document says text.
    let json = br#"[{"name":"name","type":"checkbox","value":["Yes"]}]"#;
    let (out, report) = import_form_data(&blank_form(), json, ExportFormat::Json).expect("import");
    assert_eq!(report.applied, 0);
    assert_eq!(report.mismatched.len(), 1);
    assert_eq!(report.mismatched[0].name, "name");
    assert_eq!(report.mismatched[0].expected, "checkbox");
    assert_eq!(report.mismatched[0].got, "text");
    assert!(value_of(&out, "name").iter().all(String::is_empty), "value untouched");
}

#[test]
fn multiple_values_into_a_single_value_field_is_a_mismatch() {
    let json = br#"[{"name":"fruit","type":"combo","value":["Apple","Banana"]}]"#;
    let (out, report) = import_form_data(&blank_form(), json, ExportFormat::Json).expect("import");
    assert_eq!(report.applied, 0);
    assert_eq!(report.mismatched.len(), 1);
    assert_eq!(report.mismatched[0].expected, "2 values");
    assert!(value_of(&out, "fruit").iter().all(String::is_empty));
}

#[test]
fn button_value_outside_the_fields_states_is_a_mismatch() {
    let json = br#"[{"name":"color","type":"radio","value":["Purple"]}]"#;
    let (_, report) = import_form_data(&blank_form(), json, ExportFormat::Json).expect("import");
    assert_eq!(report.applied, 0);
    assert_eq!(report.mismatched[0].expected, "state Purple");
}

#[test]
fn choice_value_outside_opt_is_rejected_by_the_setter() {
    // The choice setter is the authority on /Opt membership (P5.A4); import
    // surfaces its rejection rather than writing an undeclared value.
    let json = br#"[{"name":"fruit","type":"combo","value":["Cherry"]}]"#;
    let err = import_form_data(&blank_form(), json, ExportFormat::Json).unwrap_err();
    assert!(matches!(err, CommandError::InvalidInput(_)), "got {err:?}");
}

#[test]
fn a_signature_datum_is_never_applied() {
    let out = add_field(&read("hello.pdf"), 0, [72.0, 700.0, 260.0, 724.0], "sig", &NewFieldKind::Signature)
        .expect("sig");
    let json = br#"[{"name":"sig","type":"signature","value":["scribble"]}]"#;
    let (_, report) = import_form_data(&out, json, ExportFormat::Json).expect("import");
    assert_eq!(report.applied, 0);
    assert_eq!(report.mismatched.len(), 1, "{report:?}");
}

#[test]
fn checkbox_off_clears_the_field() {
    let json = br#"[{"name":"agree","type":"checkbox","value":["Off"]}]"#;
    let (out, report) = import_form_data(&filled_form(), json, ExportFormat::Json).expect("import");
    assert_eq!(report.applied, 1);
    assert_eq!(value_of(&out, "agree"), vec!["Off".to_owned()]);
}

#[test]
fn csv_quoted_comma_survives_the_round_trip() {
    let filled =
        vibepdf_lib::pdf::form::set_text_field_value(&blank_form(), "name", "Lovelace, Ada")
            .expect("name");
    let csv = serialize(&data_of(&filled), ExportFormat::Csv).expect("csv");
    let (out, report) = import_form_data(&blank_form(), &csv, ExportFormat::Csv).expect("import");
    assert!(report.mismatched.is_empty(), "{report:?}");
    assert_eq!(value_of(&out, "name"), vec!["Lovelace, Ada".to_owned()]);
}

#[test]
fn unicode_value_survives_json_import() {
    let filled = vibepdf_lib::pdf::form::set_text_field_value(&blank_form(), "name", "Ådàm café")
        .expect("name");
    let json = serialize(&data_of(&filled), ExportFormat::Json).expect("json");
    let (out, _) = import_form_data(&blank_form(), &json, ExportFormat::Json).expect("import");
    assert_eq!(value_of(&out, "name"), vec!["Ådàm café".to_owned()]);
}

#[test]
fn malformed_data_is_an_input_error_not_a_panic() {
    for (bytes, fmt) in [
        (&b"not json at all"[..], ExportFormat::Json),
        (&b"<html/>"[..], ExportFormat::Xfdf),
        (&b"just some text"[..], ExportFormat::Fdf),
    ] {
        let err = import_form_data(&blank_form(), bytes, fmt).unwrap_err();
        assert!(matches!(err, CommandError::InvalidInput(_)), "{fmt:?} gave {err:?}");
    }
}

#[test]
fn importing_into_a_form_less_pdf_reports_everything_unmatched() {
    let json = br#"[{"name":"name","type":"text","value":["Ada"]}]"#;
    let (_, report) = import_form_data(&read("hello.pdf"), json, ExportFormat::Json).expect("import");
    assert_eq!(report.applied, 0);
    assert_eq!(report.unmatched, vec!["name".to_owned()]);
}

#[test]
#[ignore = "produces a verification artifact; run on demand"]
fn writes_verification_artifact() {
    // A blank form with the exported data imported back in — should open with
    // every field filled *and* still interactive in any reader.
    let out = round_trip(ExportFormat::Json);
    let path = std::env::temp_dir().join("vibepdf-verify-form-import.pdf");
    std::fs::write(&path, out).expect("write");
    eprintln!("wrote {}", path.display());
}

#[tokio::test]
async fn imports_through_the_actor_and_undoes() {
    let src = std::env::temp_dir().join(format!("vibepdf-c2-{}.pdf", uuid::Uuid::new_v4()));
    std::fs::write(&src, blank_form()).expect("write");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, src, None).expect("spawn");

    let data = std::env::temp_dir().join(format!("vibepdf-c2-{}.json", uuid::Uuid::new_v4()));
    std::fs::write(&data, serialize(&data_of(&filled_form()), ExportFormat::Json).expect("json"))
        .expect("write data");

    let outcome = handle.import_form_data(ExportFormat::Json, data).await.expect("import");
    assert_eq!(outcome.report.applied, 5);
    assert!(outcome.history.can_undo);
    let after = handle.get_bytes().await.expect("bytes");
    assert_eq!(value_of(&after, "name"), vec!["Ada".to_owned()]);

    handle.undo().await.expect("undo");
    let restored = handle.get_bytes().await.expect("bytes");
    assert!(value_of(&restored, "name").iter().all(String::is_empty), "undo restored the blank");
    drop(handle);
}
