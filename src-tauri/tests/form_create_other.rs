//! Integration tests for creating non-text fields (P5.B2).
//!
//! SPEC: P5-FORM-007 — create checkbox, radio (grouped), combo, list, signature,
//! and push-button fields. Verified by re-reading through A3/A4's readers and by
//! inspecting `/FT` / `/Ff` at the COS level.

use lopdf::{Document, Object};

use vibepdf_lib::error::CommandError;
use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::form::{
    add_field, read_button_fields, read_choice_fields, read_form_summary, read_page_fields,
    NewFieldKind,
};

fn fixture(name: &str) -> std::path::PathBuf {
    let p = std::path::PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).expect("read fixture")
}

const RECT: [f32; 4] = [72.0, 620.0, 260.0, 700.0];

fn opts(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn creates_checkbox() {
    let out = add_field(&read("hello.pdf"), 0, RECT, "agree", &NewFieldKind::Checkbox { required: false })
        .expect("add");
    let b = read_button_fields(&out, 0).expect("read");
    let cb = b.iter().find(|x| x.field_name == "agree").expect("checkbox");
    assert_eq!(cb.kind, "checkbox");
    assert_eq!(cb.on_state, "Yes");
}

#[test]
fn creates_radio_group() {
    let out = add_field(&read("hello.pdf"), 0, RECT, "color", &NewFieldKind::RadioGroup {
        options: opts(&["Red", "Green", "Blue"]),
    })
    .expect("add");
    let radios: Vec<_> = read_button_fields(&out, 0).expect("read").into_iter().filter(|x| x.field_name == "color").collect();
    assert_eq!(radios.len(), 3);
    assert!(radios.iter().all(|r| r.kind == "radio"));
    let states: Vec<&str> = radios.iter().map(|r| r.on_state.as_str()).collect();
    assert!(states.contains(&"Red") && states.contains(&"Green") && states.contains(&"Blue"), "{states:?}");
}

#[test]
fn radio_needs_two_options() {
    let err = add_field(&read("hello.pdf"), 0, RECT, "x", &NewFieldKind::RadioGroup { options: opts(&["only"]) })
        .unwrap_err();
    assert!(matches!(err, CommandError::InvalidInput(_)), "got {err:?}");
}

#[test]
fn creates_combo() {
    let out = add_field(&read("hello.pdf"), 0, RECT, "fruit", &NewFieldKind::Combo {
        options: opts(&["Apple", "Banana"]),
        default: "Apple".to_owned(),
    })
    .expect("add");
    let c = read_choice_fields(&out, 0).expect("read").into_iter().find(|x| x.name == "fruit").unwrap();
    assert_eq!(c.kind, "combo");
    assert_eq!(c.options.len(), 2);
    assert_eq!(c.selected, vec!["Apple".to_owned()]);
}

#[test]
fn creates_list_multiselect() {
    let out = add_field(&read("hello.pdf"), 0, RECT, "colors", &NewFieldKind::ListBox {
        options: opts(&["Red", "Green"]),
        multi: true,
    })
    .expect("add");
    let l = read_choice_fields(&out, 0).expect("read").into_iter().find(|x| x.name == "colors").unwrap();
    assert_eq!(l.kind, "list");
    assert!(l.multi);
}

#[test]
fn creates_signature() {
    let out = add_field(&read("hello.pdf"), 0, RECT, "sig", &NewFieldKind::Signature).expect("add");
    assert_eq!(read_form_summary(&out).expect("summary").field_count, 1);
    assert_eq!(field_ft(&out, "sig").as_deref(), Some("Sig"));
}

#[test]
fn creates_pushbutton() {
    let out = add_field(&read("hello.pdf"), 0, RECT, "submit", &NewFieldKind::PushButton {
        caption: "Submit".to_owned(),
    })
    .expect("add");
    // Pushbuttons carry no value → excluded from the fillable button read.
    assert!(read_button_fields(&out, 0).expect("read").is_empty());
    assert_ne!(field_ff(&out, "submit") & (1 << 16), 0, "pushbutton /Ff bit set");
}

#[test]
fn rejects_duplicate_name() {
    let err = add_field(&read("forms.pdf"), 0, RECT, "name", &NewFieldKind::Checkbox { required: false })
        .unwrap_err();
    assert!(matches!(err, CommandError::InvalidInput(_)), "got {err:?}");
}

#[tokio::test]
async fn create_then_undo() {
    let id = uuid::Uuid::new_v4();
    let handle = DocumentActorHandle::spawn(None, id, fixture("hello.pdf"), None).expect("spawn");
    handle
        .add_field(0, RECT, "agree".to_owned(), NewFieldKind::Checkbox { required: false })
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
    let out = add_field(&read("hello.pdf"), 0, [72.0, 700.0, 90.0, 718.0], "agree", &NewFieldKind::Checkbox { required: false }).expect("cb");
    let out = add_field(&out, 0, [72.0, 600.0, 260.0, 680.0], "color", &NewFieldKind::RadioGroup { options: opts(&["Red", "Green"]) }).expect("radio");
    let out = add_field(&out, 0, [72.0, 560.0, 260.0, 584.0], "fruit", &NewFieldKind::Combo { options: opts(&["Apple", "Banana"]), default: String::new() }).expect("combo");
    let out = add_field(&out, 0, [300.0, 700.0, 460.0, 724.0], "submit", &NewFieldKind::PushButton { caption: "Submit".to_owned() }).expect("btn");
    // All six kinds, so the artifact exercises every P5-sweep appearance fix:
    // a circular radio mark, a caption inside the button, a visible (dashed)
    // signature placeholder, and a list box grown to fit its options.
    let out = add_field(&out, 0, [72.0, 440.0, 260.0, 458.0], "tags", &NewFieldKind::ListBox { options: opts(&["urgent", "review", "archive"]), multi: true }).expect("list");
    let out = add_field(&out, 0, [300.0, 600.0, 500.0, 660.0], "sign", &NewFieldKind::Signature).expect("sig");
    let path = std::env::temp_dir().join("vibepdf-verify.pdf");
    std::fs::write(&path, &out).expect("write artifact");
    eprintln!("wrote {}", path.display());
}

/// Read a top-level field's `/FT` name (fixture fields are flat / findable by /T).
fn field_ft(bytes: &[u8], name: &str) -> Option<String> {
    field_entry(bytes, name, b"FT").and_then(|o| o.as_name().ok().map(|n| String::from_utf8_lossy(n).into_owned()))
}

fn field_ff(bytes: &[u8], name: &str) -> i64 {
    field_entry(bytes, name, b"Ff").and_then(|o| o.as_i64().ok()).unwrap_or(0)
}

fn field_entry(bytes: &[u8], name: &str, key: &[u8]) -> Option<Object> {
    let doc = Document::load_mem(bytes).expect("load");
    for obj in doc.objects.values() {
        if let Object::Dictionary(d) = obj {
            if d.get(b"T").and_then(Object::as_str).ok().map(|t| String::from_utf8_lossy(t)) == Some(name.into()) {
                return d.get(key).ok().cloned();
            }
        }
    }
    None
}

// ── P5 sweep fixes (B1, B4, B5, B6, B7, B8) ─────────────────────────────────
//
// Each of these encodes a defect the first in-app sweep of Phase 5 surfaced.

/// B1 — a radio option's widget is a SQUARE, not the full dragged width. A wide,
/// short drag used to stretch the mark into a flat bar.
#[test]
fn radio_options_are_square_regardless_of_drag_shape() {
    // 300pt wide, 60pt tall, 3 options → rows are 20pt, so each button is 20pt.
    let out = add_field(
        &read("hello.pdf"),
        0,
        [72.0, 600.0, 372.0, 660.0],
        "colour",
        &NewFieldKind::RadioGroup { options: opts(&["Red", "Green", "Blue"]) },
    )
    .expect("radio");

    let doc = Document::load_mem(&out).expect("load");
    let mut seen = 0;
    for obj in doc.objects.values() {
        let Ok(d) = obj.as_dict() else { continue };
        if d.get(b"Parent").is_err() || d.get(b"AP").is_err() {
            continue;
        }
        let Ok(r) = d.get(b"Rect").and_then(Object::as_array) else { continue };
        let n: Vec<f32> = r
            .iter()
            .filter_map(|o| match o {
                Object::Real(v) => Some(*v),
                Object::Integer(i) => Some(*i as f32),
                _ => None,
            })
            .collect();
        assert_eq!(n.len(), 4);
        let (w, h) = (n[2] - n[0], n[3] - n[1]);
        assert!((w - h).abs() < 0.01, "option widget {w}x{h} is not square");
        assert!(w <= 20.5, "option widget {w} wider than its row");
        seen += 1;
    }
    assert_eq!(seen, 3, "three option widgets");
}

/// B1 — the option's appearance is a circle (Bézier arcs), not the checkbox rect.
#[test]
fn radio_appearance_is_drawn_with_curves() {
    let out = add_field(&read("hello.pdf"), 0, [72.0, 600.0, 200.0, 660.0], "colour",
        &NewFieldKind::RadioGroup { options: opts(&["Red", "Green"]) }).expect("radio");
    let doc = Document::load_mem(&out).expect("load");
    let streams: String = doc
        .objects
        .values()
        .filter_map(|o| o.as_stream().ok())
        .map(|s| String::from_utf8_lossy(&s.content).into_owned())
        .collect();
    assert!(streams.contains(" c"), "circle uses curveto: {streams}");
}

/// B4 — a default outside the option list is rejected, not silently dropped.
#[test]
fn combo_default_outside_options_is_rejected() {
    let err = add_field(
        &read("hello.pdf"),
        0,
        [72.0, 600.0, 300.0, 624.0],
        "fruit",
        &NewFieldKind::Combo { options: opts(&["Apple", "Banana"]), default: "Cherry".to_owned() },
    )
    .unwrap_err();
    assert!(matches!(err, CommandError::InvalidInput(_)), "got {err:?}");
}

#[test]
fn combo_default_inside_options_is_kept() {
    let out = add_field(&read("hello.pdf"), 0, [72.0, 600.0, 300.0, 624.0], "fruit",
        &NewFieldKind::Combo { options: opts(&["Apple", "Banana"]), default: "Banana".to_owned() })
        .expect("combo");
    let doc = Document::load_mem(&out).expect("load");
    let data = vibepdf_lib::pdf::form_data::collect_form_data(&doc).expect("collect");
    assert_eq!(data[0].value, vec!["Banana".to_owned()]);
}

/// B5 — a list box dragged too short grows to fit its options.
#[test]
fn list_box_grows_to_fit_its_options() {
    // 18pt tall drag, 5 options → must end up tall enough for all five rows.
    let out = add_field(&read("hello.pdf"), 0, [72.0, 600.0, 300.0, 618.0], "tags",
        &NewFieldKind::ListBox { options: opts(&["a", "b", "c", "d", "e"]), multi: true })
        .expect("list");
    let fields = read_page_fields(&out, 0).expect("page fields");
    let r = fields.iter().find(|f| f.name == "tags").expect("tags").rect;
    assert!(r[3] - r[1] >= 5.0 * 14.0, "height {} too short for 5 rows", r[3] - r[1]);
    assert!((r[3] - 618.0).abs() < 0.01, "grows downward — top edge is preserved");
}

#[test]
fn a_tall_enough_list_box_is_left_alone() {
    let out = add_field(&read("hello.pdf"), 0, [72.0, 400.0, 300.0, 600.0], "tags",
        &NewFieldKind::ListBox { options: opts(&["a", "b"]), multi: false }).expect("list");
    let fields = read_page_fields(&out, 0).expect("page fields");
    let r = fields.iter().find(|f| f.name == "tags").expect("tags").rect;
    assert!((r[1] - 400.0).abs() < 0.01, "not shrunk: {r:?}");
}

/// B6 — a signature field draws a placeholder, so it isn't invisible.
#[test]
fn signature_field_has_a_visible_placeholder_appearance() {
    let out = add_field(&read("hello.pdf"), 0, [72.0, 600.0, 300.0, 660.0], "sig",
        &NewFieldKind::Signature).expect("sig");
    let doc = Document::load_mem(&out).expect("load");
    let has_ap = doc.objects.values().any(|o| {
        o.as_dict().is_ok_and(|d| {
            d.get(b"FT").and_then(Object::as_name).is_ok_and(|n| n == b"Sig") && d.get(b"AP").is_ok()
        })
    });
    assert!(has_ap, "signature widget carries an /AP");
}

/// B8 — the push-button's caption is drawn into its appearance, not only stored
/// in `/MK /CA` (nothing renders that for us).
#[test]
fn pushbutton_caption_is_drawn_into_the_appearance() {
    let out = add_field(&read("hello.pdf"), 0, [72.0, 600.0, 200.0, 630.0], "go",
        &NewFieldKind::PushButton { caption: "Submit".to_owned() }).expect("button");
    let doc = Document::load_mem(&out).expect("load");
    let streams: String = doc
        .objects
        .values()
        .filter_map(|o| o.as_stream().ok())
        .map(|s| String::from_utf8_lossy(&s.content).into_owned())
        .collect();
    assert!(streams.contains("(Submit) Tj"), "caption drawn: {streams}");
}

/// B7 — the duplicate-name guard now sees names nested under `/Kids`, not just
/// the top-level `/Fields` entries.
#[test]
fn duplicate_name_nested_in_kids_is_rejected() {
    // A radio group's *parent* holds the name; it lives under /Fields, but the
    // qualified names of hierarchical children used to be invisible to the guard.
    let out = add_field(&read("hello.pdf"), 0, [72.0, 600.0, 200.0, 660.0], "colour",
        &NewFieldKind::RadioGroup { options: opts(&["Red", "Green"]) }).expect("radio");
    let err = add_field(&out, 0, [72.0, 500.0, 200.0, 524.0], "colour",
        &NewFieldKind::Checkbox { required: false }).unwrap_err();
    assert!(matches!(err, CommandError::InvalidInput(_)), "got {err:?}");
}
