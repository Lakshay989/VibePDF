//! Integration tests for form flatten (P5.C2).
//!
//! SPEC: P5-FORM-010 — "WHEN the user flattens a form, THE system SHALL render
//! each field's current appearance into the page content and remove the
//! interactive field definitions."
//!
//! The first test is the reason this module exists at all: filling a text field
//! deletes its `/AP` and sets `/NeedAppearances`, so a flatten that only baked
//! existing appearances would drop every typed value on the floor.

use lopdf::{Document, Object};

use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::form::{
    add_field, add_text_field, read_form_summary, set_button_field, set_choice_field,
    set_text_field_value, NewFieldKind,
};
use vibepdf_lib::pdf::form_flatten::flatten_form;

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

/// Every page's decoded content streams, concatenated — what a viewer draws.
fn page_content(bytes: &[u8]) -> String {
    let doc = Document::load_mem(bytes).expect("load");
    let mut out = String::new();
    for (_, page_id) in doc.get_pages() {
        let data = doc.get_page_content(page_id).expect("content");
        out.push_str(&String::from_utf8_lossy(&data));
    }
    out
}

/// The `/AP` form XObjects the page's content references, decoded — flatten
/// registers each baked appearance under `/Resources /XObject` and `Do`s it, so
/// the drawn text lives in those streams rather than in `/Contents` itself.
fn baked_text(bytes: &[u8]) -> String {
    let doc = Document::load_mem(bytes).expect("load");
    let mut out = String::new();
    for (_, page_id) in doc.get_pages() {
        let Ok((_, res_ids)) = doc.get_page_resources(page_id) else { continue };
        let dicts: Vec<lopdf::Dictionary> = res_ids
            .iter()
            .filter_map(|id| doc.get_object(*id).ok()?.as_dict().ok().cloned())
            .chain(
                doc.get_dictionary(page_id)
                    .ok()
                    .and_then(|p| p.get(b"Resources").ok())
                    .and_then(|o| match o {
                        Object::Dictionary(d) => Some(d.clone()),
                        Object::Reference(id) => doc.get_dictionary(*id).ok().cloned(),
                        _ => None,
                    }),
            )
            .collect();
        for res in dicts {
            let Ok(xo) = res.get(b"XObject").and_then(Object::as_dict) else { continue };
            for (_, v) in xo.iter() {
                let Ok(id) = v.as_reference() else { continue };
                if let Ok(stream) = doc.get_object(id).and_then(Object::as_stream) {
                    let data = stream.decompressed_content().unwrap_or_else(|_| stream.content.clone());
                    out.push_str(&String::from_utf8_lossy(&data));
                }
            }
        }
    }
    out
}

fn has_acroform(bytes: &[u8]) -> bool {
    let doc = Document::load_mem(bytes).expect("load");
    let Ok(root) = doc.trailer.get(b"Root").and_then(Object::as_reference) else { return false };
    doc.get_dictionary(root).is_ok_and(|c| c.get(b"AcroForm").is_ok())
}

fn widget_count(bytes: &[u8]) -> usize {
    let doc = Document::load_mem(bytes).expect("load");
    doc.get_pages()
        .values()
        .map(|&page_id| {
            let annots = match doc.get_dictionary(page_id).ok().and_then(|p| p.get(b"Annots").ok()) {
                Some(Object::Array(a)) => a.clone(),
                Some(Object::Reference(id)) => {
                    doc.get_object(*id).and_then(Object::as_array).cloned().unwrap_or_default()
                }
                _ => Vec::new(),
            };
            annots
                .iter()
                .filter(|o| {
                    o.as_reference()
                        .ok()
                        .and_then(|id| doc.get_dictionary(id).ok())
                        .and_then(|d| d.get(b"Subtype").and_then(Object::as_name).ok())
                        == Some(b"Widget")
                })
                .count()
        })
        .sum()
}

/// A filled form: text, checkbox, radio, combo, multi-select list.
fn filled_form() -> Vec<u8> {
    let out = add_text_field(&read("hello.pdf"), 0, [72.0, 700.0, 260.0, 724.0], "name", "", None, false, false)
        .expect("text");
    let out = add_field(&out, 0, [72.0, 660.0, 90.0, 678.0], "agree", &NewFieldKind::Checkbox { required: false })
        .expect("checkbox");
    let out = add_field(&out, 0, [72.0, 560.0, 260.0, 640.0], "color", &NewFieldKind::RadioGroup { options: opts(&["Red", "Green"]) })
        .expect("radio");
    let out = add_field(&out, 0, [72.0, 520.0, 260.0, 544.0], "fruit", &NewFieldKind::Combo { options: opts(&["Apple", "Banana"]), default: String::new() })
        .expect("combo");
    let out = set_text_field_value(&out, "name", "Ada Lovelace").expect("fill name");
    let out = set_button_field(&out, "agree", "Yes", true).expect("check");
    let out = set_button_field(&out, "color", "Green", true).expect("radio");
    set_choice_field(&out, "fruit", &["Banana".to_owned()]).expect("combo")
}

#[test]
fn a_filled_text_value_survives_the_flatten() {
    // The regression this whole module guards: filling deletes /AP, so flatten
    // must synthesize one from /V or the typed text disappears.
    let flat = flatten_form(&filled_form()).expect("flatten");
    assert!(baked_text(&flat).contains("Ada Lovelace"), "value baked into an /AP form");
}

#[test]
fn a_selected_choice_label_survives_the_flatten() {
    let flat = flatten_form(&filled_form()).expect("flatten");
    assert!(baked_text(&flat).contains("Banana"), "selected option baked");
}

#[test]
fn the_acroform_is_gone_and_no_widgets_remain() {
    let flat = flatten_form(&filled_form()).expect("flatten");
    assert!(!has_acroform(&flat), "interactive field definitions removed");
    assert_eq!(widget_count(&flat), 0, "no widget annotations left");
    assert_eq!(read_form_summary(&flat).expect("summary").field_count, 0);
}

#[test]
fn the_form_had_fields_before_flattening() {
    // Guards the assertion above from passing vacuously.
    let filled = filled_form();
    assert!(has_acroform(&filled));
    assert!(widget_count(&filled) >= 4, "widgets: {}", widget_count(&filled));
    assert_eq!(read_form_summary(&filled).expect("summary").field_count, 4);
}

#[test]
fn a_checked_button_bakes_its_on_appearance() {
    // The "on" face is a filled rect (`re f`); "off" is an empty stream. The
    // /AS-selected state is what flatten must pick up.
    let flat = flatten_form(&filled_form()).expect("flatten");
    assert!(page_content(&flat).contains("Do"), "page draws the baked appearances");
    assert!(baked_text(&flat).contains("re f"), "the checked face is in a baked form");
}

#[test]
fn an_unchecked_checkbox_bakes_nothing_visible_but_still_disappears() {
    let out = add_field(&read("hello.pdf"), 0, [72.0, 660.0, 90.0, 678.0], "agree", &NewFieldKind::Checkbox { required: false })
        .expect("checkbox");
    let flat = flatten_form(&out).expect("flatten");
    assert!(!baked_text(&flat).contains("re f"), "the off face draws nothing");
    assert_eq!(widget_count(&flat), 0);
    assert!(!has_acroform(&flat));
}

#[test]
fn non_widget_annotations_stay_live() {
    // A sticky note alongside the form must survive as an annotation.
    let with_note =
        vibepdf_lib::pdf::cos::add_text_note(&filled_form(), "n1", 0, 300.0, 700.0, "keep me", "Ada")
            .expect("note");
    let flat = flatten_form(&with_note).expect("flatten");
    assert_eq!(widget_count(&flat), 0, "widgets gone");
    let doc = Document::load_mem(&flat).expect("load");
    let page_id = *doc.get_pages().values().next().expect("page");
    let annots = doc.get_dictionary(page_id).expect("page").get(b"Annots");
    assert!(annots.is_ok(), "the note is still an annotation");
}

#[test]
fn a_hidden_field_leaves_no_content_behind() {
    let out = add_text_field(&read("hello.pdf"), 0, [72.0, 700.0, 260.0, 724.0], "secret", "hidden value", None, false, false)
        .expect("text");
    // Mark every widget hidden (/F bit 2).
    let mut doc = Document::load_mem(&out).expect("load");
    let ids: Vec<lopdf::ObjectId> = doc
        .objects
        .iter()
        .filter(|(_, o)| {
            o.as_dict().is_ok_and(|d| {
                d.get(b"Subtype").and_then(Object::as_name).is_ok_and(|n| n == b"Widget")
            })
        })
        .map(|(id, _)| *id)
        .collect();
    for id in ids {
        if let Ok(d) = doc.get_dictionary_mut(id) {
            d.set("F", Object::Integer(2));
        }
    }
    let mut hidden = Vec::new();
    doc.save_to(&mut hidden).expect("save");

    let flat = flatten_form(&hidden).expect("flatten");
    assert!(!baked_text(&flat).contains("hidden value"), "a hidden field paints nothing");
    assert_eq!(widget_count(&flat), 0);
}

#[test]
fn a_form_less_pdf_is_left_alone() {
    let flat = flatten_form(&read("hello.pdf")).expect("flatten");
    let before = Document::load_mem(&read("hello.pdf")).expect("load").get_pages().len();
    assert_eq!(Document::load_mem(&flat).expect("load").get_pages().len(), before);
}

#[test]
fn xfa_goes_with_the_acroform() {
    let bytes = read("forms-xfa.pdf");
    let flat = flatten_form(&bytes).expect("flatten");
    assert!(!has_acroform(&flat), "/AcroForm and its /XFA both removed");
    assert!(!read_form_summary(&flat).expect("summary").has_xfa);
}

#[test]
fn a_non_latin_value_bakes_through_the_cid_path() {
    let out = add_text_field(&read("hello.pdf"), 0, [72.0, 700.0, 300.0, 724.0], "who", "", None, false, false)
        .expect("text");
    let out = set_text_field_value(&out, "who", "Ådàm café").expect("fill");
    // Non-WinAnsi text takes the embedded-CID branch; the assertion is that it
    // produces a bakeable appearance at all rather than erroring out.
    let flat = flatten_form(&out).expect("flatten");
    assert!(page_content(&flat).contains("Do"), "an appearance was baked");
    assert_eq!(widget_count(&flat), 0);
}

#[test]
fn page_geometry_is_unchanged() {
    let before = Document::load_mem(&filled_form()).expect("load");
    let before_box = {
        let page_id = *before.get_pages().values().next().expect("page");
        before.get_dictionary(page_id).expect("page").get(b"MediaBox").ok().cloned()
    };
    let flat = flatten_form(&filled_form()).expect("flatten");
    let after = Document::load_mem(&flat).expect("load");
    let page_id = *after.get_pages().values().next().expect("page");
    assert_eq!(after.get_pages().len(), before.get_pages().len());
    assert_eq!(after.get_dictionary(page_id).expect("page").get(b"MediaBox").ok().cloned(), before_box);
}

#[tokio::test]
async fn flattens_through_the_actor_and_undoes() {
    let src = std::env::temp_dir().join(format!("vibepdf-c2f-{}.pdf", uuid::Uuid::new_v4()));
    std::fs::write(&src, filled_form()).expect("write");
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, src, None).expect("spawn");

    let state = handle.flatten_form().await.expect("flatten");
    assert!(state.can_undo);
    let after = handle.get_bytes().await.expect("bytes");
    assert_eq!(read_form_summary(&after).expect("summary").field_count, 0);

    handle.undo().await.expect("undo");
    let restored = handle.get_bytes().await.expect("bytes");
    assert_eq!(read_form_summary(&restored).expect("summary").field_count, 4, "form is back");
    drop(handle);
}

#[test]
#[ignore = "produces a verification artifact; run on demand"]
fn writes_verification_artifact() {
    let flat = flatten_form(&filled_form()).expect("flatten");
    let path = std::env::temp_dir().join("vibepdf-verify-form-flatten.pdf");
    std::fs::write(&path, flat).expect("write");
    eprintln!("wrote {}", path.display());
}
