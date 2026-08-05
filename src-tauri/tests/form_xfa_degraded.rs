//! Integration tests for XFA degraded support (P5.A5).
//!
//! SPEC: P5-FORM-005 — WHERE a PDF is XFA-only (no AcroForm fallback), warn that
//! XFA editing isn't supported and offer to convert to flat content (read-only).
//! These verify detection (XFA present, 0 fillable fields) and the convert action
//! (strip `/XFA`, set `/NeedAppearances`), including the no-XFA error path.

use lopdf::{Document, Object};

use vibepdf_lib::error::CommandError;
use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::form::{read_form_summary, strip_xfa};

fn fixture(name: &str) -> std::path::PathBuf {
    let p = std::path::PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).expect("read fixture")
}

#[test]
fn detects_xfa_only() {
    let s = read_form_summary(&read("forms-xfa.pdf")).expect("summary");
    assert!(s.has_xfa, "XFA present");
    assert_eq!(s.field_count, 0, "no fillable AcroForm fields");
}

#[test]
fn strip_removes_xfa() {
    let out = strip_xfa(&read("forms-xfa.pdf")).expect("strip");
    let s = read_form_summary(&out).expect("summary");
    assert!(!s.has_xfa, "XFA gone after strip");
}

#[test]
fn strip_sets_need_appearances() {
    let out = strip_xfa(&read("forms-xfa.pdf")).expect("strip");
    assert!(has_need_appearances(&out), "/NeedAppearances set");
}

#[test]
fn strip_without_xfa_errors() {
    let err = strip_xfa(&read("forms.pdf")).unwrap_err();
    assert!(matches!(err, CommandError::InvalidInput(_)), "got {err:?}");
}

#[tokio::test]
async fn strip_then_undo() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("forms-xfa.pdf"), None).expect("spawn");
    handle.strip_xfa().await.expect("strip");
    assert!(!handle.read_form_summary().await.expect("summary").has_xfa, "XFA gone");
    handle.undo().await.expect("undo");
    assert!(handle.read_form_summary().await.expect("summary").has_xfa, "undo restores XFA");
    drop(handle);
}

#[test]
#[ignore = "produces a verification artifact; run on demand"]
fn writes_verification_artifact() {
    let out = strip_xfa(&read("forms-xfa.pdf")).expect("strip");
    let path = std::env::temp_dir().join("vibepdf-verify.pdf");
    std::fs::write(&path, &out).expect("write artifact");
    eprintln!("wrote {}", path.display());
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
