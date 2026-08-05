//! Integration tests for filling checkbox / radio fields (P5.A3).
//!
//! SPEC: P5-FORM-003 — "WHEN the user clicks a checkbox or radio button, THE
//! system SHALL toggle/select the field per its declared appearance states."
//! These verify the read (kind, on-state, checked) and the write (field `/V` +
//! each widget's `/AS`), and that a button write does *not* force
//! `/NeedAppearances` (button appearances are pre-baked in `/AP /N`).

use lopdf::{dictionary, Document, Object};

use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::form::{read_button_fields, set_button_field};

fn fixture(name: &str) -> std::path::PathBuf {
    let p = std::path::PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).expect("read fixture")
}

#[test]
fn reads_checkbox_and_radio() {
    let b = read_button_fields(&read("forms-buttons.pdf"), 0).expect("read");
    assert_eq!(b.len(), 3);
    let cb = b.iter().find(|x| x.field_name == "agree").expect("checkbox");
    assert_eq!(cb.kind, "checkbox");
    assert_eq!(cb.on_state, "Yes");
    assert!(!cb.checked);
    let radios: Vec<_> = b.iter().filter(|x| x.field_name == "color").collect();
    assert_eq!(radios.len(), 2);
    assert!(radios.iter().all(|r| r.kind == "radio" && !r.checked));
    let states: Vec<&str> = radios.iter().map(|r| r.on_state.as_str()).collect();
    assert!(states.contains(&"Red") && states.contains(&"Green"), "states {states:?}");
}

#[test]
fn checks_and_unchecks_a_checkbox() {
    let on = set_button_field(&read("forms-buttons.pdf"), "agree", "Yes", true).expect("check");
    let cb = read_button_fields(&on, 0).expect("read").into_iter().find(|x| x.field_name == "agree").unwrap();
    assert!(cb.checked, "checkbox on after check");

    let off = set_button_field(&on, "agree", "Yes", false).expect("uncheck");
    let cb = read_button_fields(&off, 0).expect("read").into_iter().find(|x| x.field_name == "agree").unwrap();
    assert!(!cb.checked, "checkbox off after uncheck");
}

#[test]
fn selecting_radio_option_flips_siblings() {
    let out = set_button_field(&read("forms-buttons.pdf"), "color", "Green", true).expect("select");
    let radios: Vec<_> =
        read_button_fields(&out, 0).expect("read").into_iter().filter(|x| x.field_name == "color").collect();
    let green = radios.iter().find(|r| r.on_state == "Green").unwrap();
    let red = radios.iter().find(|r| r.on_state == "Red").unwrap();
    assert!(green.checked, "selected option checked (via /V)");
    assert!(!red.checked, "sibling unchecked");

    // /AS must also flip on each widget (what viewers actually draw).
    let as_states = widget_as_states(&out);
    assert_eq!(as_states.get("Green").map(String::as_str), Some("Green"), "{as_states:?}");
    assert_eq!(as_states.get("Red").map(String::as_str), Some("Off"), "{as_states:?}");
}

#[test]
fn non_yes_on_state() {
    // A checkbox whose on-state is /On (not /Yes) — discovered + settable.
    let mut doc = Document::with_version("1.5");
    let on_ap = doc.add_object(dictionary! { "Type" => "XObject", "Subtype" => "Form" });
    let off_ap = doc.add_object(dictionary! { "Type" => "XObject", "Subtype" => "Form" });
    let field = doc.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "FT" => "Btn",
        "T" => Object::string_literal("box"), "AS" => Object::Name(b"Off".to_vec()),
        "Rect" => vec![10.into(), 10.into(), 28.into(), 28.into()],
        "AP" => dictionary! { "N" => dictionary! { "On" => on_ap, "Off" => off_ap } },
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
        Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => page_tree, "AcroForm" => acro });
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("serialize");

    let b = read_button_fields(&bytes, 0).expect("read");
    assert_eq!(b[0].on_state, "On");
    let out = set_button_field(&bytes, "box", "On", true).expect("check");
    assert!(read_button_fields(&out, 0).expect("read")[0].checked);
}

#[test]
fn does_not_touch_need_appearances() {
    // forms-buttons.pdf declares no /NeedAppearances; a button write must not add it.
    assert!(!has_need_appearances(&read("forms-buttons.pdf")), "fixture starts without it");
    let out = set_button_field(&read("forms-buttons.pdf"), "agree", "Yes", true).expect("check");
    assert!(!has_need_appearances(&out), "button write left /NeedAppearances absent");
}

#[tokio::test]
async fn set_then_undo() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("forms-buttons.pdf"), None).expect("spawn");
    handle.set_button_field("agree".to_owned(), "Yes".to_owned(), true).await.expect("check");
    let on = handle.read_button_fields(0).await.expect("read");
    assert!(on.iter().find(|x| x.field_name == "agree").unwrap().checked);
    handle.undo().await.expect("undo");
    let after = handle.read_button_fields(0).await.expect("read");
    assert!(!after.iter().find(|x| x.field_name == "agree").unwrap().checked, "undo cleared it");
    drop(handle);
}

#[test]
#[ignore = "produces a verification artifact; run on demand"]
fn writes_verification_artifact() {
    let out = set_button_field(&read("forms-buttons.pdf"), "agree", "Yes", true).expect("check");
    let out = set_button_field(&out, "color", "Green", true).expect("select");
    let path = std::env::temp_dir().join("vibepdf-verify.pdf");
    std::fs::write(&path, &out).expect("write artifact");
    eprintln!("wrote {}", path.display());
}

// ── test helpers ────────────────────────────────────────────────────────────

fn resolve<'a>(doc: &'a Document, obj: &'a Object) -> Option<&'a lopdf::Dictionary> {
    match obj {
        Object::Reference(id) => doc.get_dictionary(*id).ok(),
        Object::Dictionary(d) => Some(d),
        _ => None,
    }
}

/// Map of on-state → `/AS` for every button widget on page 0.
fn widget_as_states(bytes: &[u8]) -> std::collections::HashMap<String, String> {
    let doc = Document::load_mem(bytes).expect("load");
    let mut out = std::collections::HashMap::new();
    for (page_no, page_id) in doc.get_pages() {
        if page_no != 1 {
            continue;
        }
        let Ok(annots) = doc.get_dictionary(page_id).and_then(|p| p.get(b"Annots")).and_then(Object::as_array)
        else {
            continue;
        };
        for o in annots {
            let Ok(id) = o.as_reference() else { continue };
            let Ok(d) = doc.get_dictionary(id) else { continue };
            let Some(on) = d
                .get(b"AP")
                .ok()
                .and_then(|x| resolve(&doc, x))
                .and_then(|ap| ap.get(b"N").ok().and_then(|x| resolve(&doc, x)))
                .and_then(|n| n.iter().map(|(k, _)| k.clone()).find(|k| k != b"Off"))
            else {
                continue;
            };
            let as_v = d
                .get(b"AS")
                .and_then(Object::as_name)
                .map(|x| String::from_utf8_lossy(x).into_owned())
                .unwrap_or_default();
            out.insert(String::from_utf8_lossy(&on).into_owned(), as_v);
        }
    }
    out
}

fn has_need_appearances(bytes: &[u8]) -> bool {
    let doc = Document::load_mem(bytes).expect("load");
    let Ok(root) = doc.trailer.get(b"Root").and_then(Object::as_reference) else { return false };
    let Ok(acro_obj) = doc.get_dictionary(root).and_then(|c| c.get(b"AcroForm")) else { return false };
    let Some(acro) = resolve(&doc, acro_obj) else { return false };
    matches!(acro.get(b"NeedAppearances"), Ok(Object::Boolean(true)))
}
