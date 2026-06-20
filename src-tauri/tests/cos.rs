//! Capability spike for the `lopdf` adoption (COS / object-model layer).
//!
//! Proves the three structural operations PDFium's API can't do — outline
//! read, outline write, form-field rename — and, crucially, that every lopdf
//! output **reopens cleanly in PDFium** (the cross-library byte-compatibility
//! guarantee the integration model depends on). No feature is wired to this
//! yet; it validates the dependency choice. Single-threaded (PDFium).

use std::path::PathBuf;

use lopdf::{Dictionary, Document, Object};
use vibepdf_lib::pdf::cos::{
    add_free_text, add_shape, add_text_markup, add_text_note, add_top_level_bookmark,
    clear_text_markup, delete_annotation, merge_documents, prune_dangling_destinations,
    read_annotations, read_form_field_names, read_free_text, read_text_notes,
    register_inserted_form_fields, read_top_level_outline_titles, rename_form_fields_with_suffix,
    reorder_pages, resize_pages, update_free_text, update_text_note,
};
use vibepdf_lib::pdf::document::open_pdf;

/// Strip the `/Dest` from an annotation, producing a destination-less "dead"
/// link — the shape `FPDF_ImportPages` leaves when a link's target page isn't
/// copied. (lopdf's writer strips references to *deleted* objects on save, so a
/// genuinely dangling page ref can only come from PDFium — that path is covered
/// by the delete/split integration tests.)
fn strip_dest_via_lopdf(bytes: &[u8], annot_obj: (u32, u16)) -> Vec<u8> {
    let mut doc = Document::load_mem(bytes).expect("load");
    if let Ok(annot) = doc.get_dictionary_mut(annot_obj) {
        annot.remove(b"Dest");
        annot.remove(b"A");
    }
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("save");
    out
}

/// The number of annotations on a 0-based page, read via lopdf.
fn page_annot_count(bytes: &[u8], page_index: usize) -> usize {
    let doc = Document::load_mem(bytes).expect("load");
    let Some(&page_id) = doc.get_pages().get(&(page_index as u32 + 1)) else {
        return 0;
    };
    doc.get_dictionary(page_id)
        .ok()
        .and_then(|p| p.get(b"Annots").and_then(Object::as_array).ok())
        .map_or(0, Vec::len)
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("read fixture {}: {e}", p.display()))
}

fn temp_pdf(bytes: &[u8]) -> PathBuf {
    let p = std::env::temp_dir().join(format!("vibepdf-cos-{}.pdf", uuid::Uuid::new_v4()));
    std::fs::write(&p, bytes).expect("write temp");
    p
}

/// Reopen bytes through PDFium and return the page count (proves the bytes
/// are valid to the *other* engine, not just lopdf).
fn pdfium_page_count(bytes: &[u8]) -> u32 {
    let p = temp_pdf(bytes);
    let (doc, meta) = open_pdf(&p, None).expect("pdfium reopen");
    let n = meta.page_count;
    drop(doc);
    let _ = std::fs::remove_file(&p);
    n
}

fn as_strs(v: &[String]) -> Vec<&str> {
    v.iter().map(String::as_str).collect()
}

/// SPEC: P3-ANN-001 — add_text_markup writes a standard annotation dict
/// (/Subtype, /QuadPoints, /C) AND a generated /AP appearance stream, and the
/// result reopens in PDFium.
#[test]
fn cos_add_text_markup_writes_annot_with_ap() {
    let bytes = fixture_bytes("hello.pdf");
    let quads = [[72.0_f32, 700.0, 200.0, 700.0, 72.0, 688.0, 200.0, 688.0]];
    let out = add_text_markup(&bytes, 0, "highlight", &quads, "#ffd400", 1.0).expect("markup");

    assert_eq!(pdfium_page_count(&out), 1);

    let doc = Document::load_mem(&out).expect("load");
    let page_id = *doc.get_pages().get(&1).expect("page 1");
    let annots = doc
        .get_dictionary(page_id)
        .unwrap()
        .get(b"Annots")
        .and_then(Object::as_array)
        .expect("Annots array");
    assert_eq!(annots.len(), 1);

    let annot = doc.get_dictionary(annots[0].as_reference().unwrap()).unwrap();
    assert_eq!(annot.get(b"Subtype").unwrap().as_name().unwrap(), b"Highlight");
    assert_eq!(annot.get(b"QuadPoints").and_then(Object::as_array).unwrap().len(), 8);
    assert!(annot.get(b"C").and_then(Object::as_array).is_ok(), "colour /C present");

    // /AP /N must resolve to a stream (the appearance).
    let ap = annot.get(b"AP").and_then(Object::as_dict).unwrap();
    let n = ap.get(b"N").unwrap().as_reference().unwrap();
    assert!(
        doc.get_object(n).and_then(Object::as_stream).is_ok(),
        "AP /N should be a stream"
    );
}

#[test]
fn cos_text_markup_maps_each_subtype() {
    let bytes = fixture_bytes("hello.pdf");
    let quads = [[72.0_f32, 700.0, 200.0, 700.0, 72.0, 688.0, 200.0, 688.0]];
    for (input, expected) in [
        ("highlight", &b"Highlight"[..]),
        ("underline", &b"Underline"[..]),
        ("strikethrough", &b"StrikeOut"[..]),
        ("squiggly", &b"Squiggly"[..]),
    ] {
        let out = add_text_markup(&bytes, 0, input, &quads, "#ff0000", 1.0).expect(input);
        let doc = Document::load_mem(&out).expect("load");
        let page_id = *doc.get_pages().get(&1).unwrap();
        let annot = doc
            .get_dictionary(page_id)
            .unwrap()
            .get(b"Annots")
            .and_then(Object::as_array)
            .unwrap()[0]
            .as_reference()
            .unwrap();
        let subtype = doc.get_dictionary(annot).unwrap().get(b"Subtype").unwrap().as_name().unwrap();
        assert_eq!(subtype, expected, "subtype for {input}");
    }
}

#[test]
fn cos_clears_text_markup() {
    let bytes = fixture_bytes("hello.pdf");
    let quads = [[72.0_f32, 700.0, 200.0, 700.0, 72.0, 688.0, 200.0, 688.0]];
    let with_markup = add_text_markup(&bytes, 0, "highlight", &quads, "#ffd400", 1.0).expect("add");

    let cleared = clear_text_markup(&with_markup).expect("clear");
    assert_eq!(pdfium_page_count(&cleared), 1);

    let doc = Document::load_mem(&cleared).expect("load");
    let page_id = *doc.get_pages().get(&1).unwrap();
    let empty = match doc.get_dictionary(page_id).unwrap().get(b"Annots").and_then(Object::as_array) {
        Ok(a) => a.is_empty(),
        Err(_) => true, // no /Annots at all
    };
    assert!(empty, "all markup should be removed");
}

#[test]
fn cos_text_markup_rejects_bad_input() {
    let bytes = fixture_bytes("hello.pdf");
    let quads = [[72.0_f32, 700.0, 200.0, 700.0, 72.0, 688.0, 200.0, 688.0]];
    assert!(add_text_markup(&bytes, 0, "bogus", &quads, "#ffd400", 1.0).is_err());
    assert!(add_text_markup(&bytes, 0, "highlight", &[], "#ffd400", 1.0).is_err());
    assert!(add_text_markup(&bytes, 0, "highlight", &quads, "nothex", 1.0).is_err());
}

/// SPEC: P3-ANN-002 — add_text_note writes a `/Text` annotation dict with the
/// stable `/NM`, author `/T`, body `/Contents`, `/Name /Note`, `/F 28`, and
/// deliberately NO `/AP` (the reader draws the icon from `/Name`). Reopens in
/// PDFium.
#[test]
fn cos_text_note_dict_shape() {
    let bytes = fixture_bytes("hello.pdf");
    let out = add_text_note(&bytes, "nm-1", 0, 120.0, 650.0, "hello body", "Ada").expect("note");

    assert_eq!(pdfium_page_count(&out), 1);

    let doc = Document::load_mem(&out).expect("load");
    let page_id = *doc.get_pages().get(&1).expect("page 1");
    let annots = doc
        .get_dictionary(page_id)
        .unwrap()
        .get(b"Annots")
        .and_then(Object::as_array)
        .expect("Annots array");
    assert_eq!(annots.len(), 1);

    let note = doc.get_dictionary(annots[0].as_reference().unwrap()).unwrap();
    assert_eq!(note.get(b"Subtype").unwrap().as_name().unwrap(), b"Text");
    assert_eq!(note.get(b"Name").unwrap().as_name().unwrap(), b"Note");
    assert_eq!(note.get(b"NM").and_then(Object::as_str).unwrap(), b"nm-1");
    assert_eq!(note.get(b"T").and_then(Object::as_str).unwrap(), b"Ada");
    assert_eq!(note.get(b"Contents").and_then(Object::as_str).unwrap(), b"hello body");
    assert_eq!(note.get(b"F").and_then(Object::as_i64).unwrap(), 28);
    assert!(note.get(b"AP").is_err(), "a note must NOT carry an /AP — the reader draws the icon");
}

/// SPEC: P3-ANN-002 (re-openable) — read_text_notes is the inverse of
/// add_text_note: it returns each `/Text` note in page order with the `/NM`,
/// 0-based page, `/Rect` lower-left, `/Contents`, and `/T`.
#[test]
fn cos_reads_text_notes() {
    let bytes = fixture_bytes("bookmarks.pdf"); // 6 pages
    let one = add_text_note(&bytes, "a", 0, 100.0, 700.0, "first", "Ada").expect("add 1");
    let two = add_text_note(&one, "b", 2, 50.0, 60.0, "second", "Bo").expect("add 2");

    let notes = read_text_notes(&two).expect("read");
    assert_eq!(notes.len(), 2);

    let a = notes.iter().find(|n| n.nm == "a").expect("note a");
    assert_eq!(a.page, 0);
    assert_eq!((a.x, a.y), (100.0, 700.0));
    assert_eq!(a.content, "first");
    assert_eq!(a.author, "Ada");

    let b = notes.iter().find(|n| n.nm == "b").expect("note b");
    assert_eq!(b.page, 2);
    assert_eq!((b.x, b.y), (50.0, 60.0));
    assert_eq!(b.content, "second");
    assert_eq!(b.author, "Bo");
}

#[test]
fn cos_reads_no_notes_from_plain_pdf() {
    assert!(read_text_notes(&fixture_bytes("hello.pdf")).expect("read").is_empty());
}

#[test]
fn cos_note_update_and_delete_by_nm() {
    let bytes = fixture_bytes("hello.pdf");
    let with_note = add_text_note(&bytes, "n", 0, 120.0, 650.0, "before", "A").expect("add");

    let updated = update_text_note(&with_note, "n", "after").expect("update");
    let doc = Document::load_mem(&updated).expect("load");
    let page_id = *doc.get_pages().get(&1).unwrap();
    let note = doc
        .get_dictionary(doc.get_dictionary(page_id).unwrap().get(b"Annots").and_then(Object::as_array).unwrap()[0].as_reference().unwrap())
        .unwrap();
    assert_eq!(note.get(b"Contents").and_then(Object::as_str).unwrap(), b"after");

    let deleted = delete_annotation(&updated, "n").expect("delete");
    assert_eq!(page_annot_count(&deleted, 0), 0, "the note is gone after delete");
    assert_eq!(pdfium_page_count(&deleted), 1);
}

/// The single annotation dict on page 1 of `bytes`, plus its `/AP /N` stream
/// content + the BaseFont of its appearance font resource.
fn first_annot_with_ap(bytes: &[u8]) -> (Dictionary, String, String) {
    let doc = Document::load_mem(bytes).expect("load");
    let page_id = *doc.get_pages().get(&1).expect("page 1");
    let annot_ref = doc
        .get_dictionary(page_id)
        .unwrap()
        .get(b"Annots")
        .and_then(Object::as_array)
        .unwrap()[0]
        .as_reference()
        .unwrap();
    let annot = doc.get_dictionary(annot_ref).unwrap().clone();
    let n = annot
        .get(b"AP")
        .and_then(Object::as_dict)
        .unwrap()
        .get(b"N")
        .unwrap()
        .as_reference()
        .unwrap();
    let stream = doc.get_object(n).and_then(Object::as_stream).unwrap();
    let content = String::from_utf8_lossy(&stream.content).into_owned();
    let base_font = stream
        .dict
        .get(b"Resources")
        .and_then(Object::as_dict)
        .unwrap()
        .get(b"Font")
        .and_then(Object::as_dict)
        .unwrap()
        .get(b"F1")
        .and_then(Object::as_dict)
        .unwrap()
        .get(b"BaseFont")
        .and_then(Object::as_name)
        .map(|n| String::from_utf8_lossy(n).into_owned())
        .unwrap();
    (annot, content, base_font)
}

/// SPEC: P3-ANN-003 — add_free_text writes a `/FreeText` dict (`/Contents`,
/// `/DA`) plus a generated `/AP` whose stream draws the text (`BT … Tj … ET`),
/// and the result reopens in PDFium.
#[test]
fn cos_add_free_text_writes_annot_with_ap() {
    let bytes = fixture_bytes("hello.pdf");
    let out = add_free_text(&bytes, 0, [100.0, 600.0, 300.0, 700.0], "Hello", "Helvetica", 14.0, "#ff0000", false, false)
        .expect("free text");

    assert_eq!(pdfium_page_count(&out), 1);

    let (annot, content, base_font) = first_annot_with_ap(&out);
    assert_eq!(annot.get(b"Subtype").unwrap().as_name().unwrap(), b"FreeText");
    assert_eq!(annot.get(b"Contents").and_then(Object::as_str).unwrap(), b"Hello");
    assert!(annot.get(b"DA").is_ok(), "a /DA default appearance is present");
    assert!(content.contains("BT") && content.contains("Tj") && content.contains("ET"), "{content}");
    assert!(content.contains("(Hello) Tj"), "draws the text: {content}");
    assert_eq!(base_font, "Helvetica");
}

#[test]
fn cos_free_text_font_variants() {
    let bytes = fixture_bytes("hello.pdf");
    for (family, bold, italic, expected) in [
        ("Helvetica", true, false, "Helvetica-Bold"),
        ("Times", true, true, "Times-BoldItalic"),
        ("Times", false, false, "Times-Roman"),
        ("Courier", false, true, "Courier-Oblique"),
    ] {
        let out = add_free_text(&bytes, 0, [10.0, 10.0, 200.0, 40.0], "x", family, 12.0, "#000000", bold, italic)
            .unwrap_or_else(|e| panic!("{family} b={bold} i={italic}: {e}"));
        let (_, _, base_font) = first_annot_with_ap(&out);
        assert_eq!(base_font, expected, "{family} bold={bold} italic={italic}");
    }
}

#[test]
fn cos_free_text_escapes_and_splits_lines() {
    let bytes = fixture_bytes("hello.pdf");
    let out = add_free_text(&bytes, 0, [10.0, 10.0, 200.0, 80.0], "a(b)\\c\nsecond", "Helvetica", 12.0, "#000000", false, false)
        .expect("free text");
    let (_, content, _) = first_annot_with_ap(&out);
    // Parens + backslash escaped in the literal string.
    assert!(content.contains("(a\\(b\\)\\\\c) Tj"), "escaping: {content}");
    // The newline becomes a T* before the second line.
    assert!(content.contains("T*"), "newline → T*: {content}");
    assert!(content.contains("(second) Tj"), "second line: {content}");
}

/// SPEC: P3-ANN-003 — a font taller than the dragged box grows the box downward
/// (top edge fixed) so the `/AP` (clipped to `BBox == Rect`) doesn't cut text off.
#[test]
fn cos_free_text_grows_box_to_fit_large_font() {
    let bytes = fixture_bytes("hello.pdf");
    // 48pt over two lines in a 20pt-tall box (y 680..700) would clip badly.
    let out = add_free_text(&bytes, 0, [100.0, 680.0, 300.0, 700.0], "Big\nText", "Helvetica", 48.0, "#000000", false, false)
        .expect("free text");
    let doc = Document::load_mem(&out).expect("load");
    let page_id = *doc.get_pages().get(&1).unwrap();
    let annot_ref = doc
        .get_dictionary(page_id)
        .unwrap()
        .get(b"Annots")
        .and_then(Object::as_array)
        .unwrap()[0]
        .as_reference()
        .unwrap();
    let rect = doc.get_dictionary(annot_ref).unwrap().get(b"Rect").and_then(Object::as_array).unwrap();
    let (y0, y1) = (num(&rect[1]), num(&rect[3]));
    // 2 lines × 48×1.2 leading ≈ 115pt — the box must have grown well past 20pt.
    assert!(y1 - y0 >= 110.0, "box should grow to fit the text: height {}", y1 - y0);
    // The top edge stays where the user dragged it.
    assert!((y1 - 700.0).abs() < 0.5, "top fixed at 700, got {y1}");
}

/// SPEC: P3-ANN-013 — read_free_text round-trips the text + style our writer
/// stamped (so the editor opens faithfully pre-filled).
#[test]
fn cos_read_free_text_round_trips_style() {
    let bytes = fixture_bytes("hello.pdf");
    let out = add_free_text(&bytes, 0, [100.0, 560.0, 300.0, 640.0], "Hi\nThere", "Times", 18.0, "#cc1133", true, false)
        .expect("free text");
    let nm = read_annotations(&out).expect("read")[0].id.clone();

    let data = read_free_text(&out, &nm).expect("read ft").expect("some");
    assert_eq!(data.text, "Hi\nThere");
    assert_eq!(data.font_family, "Times");
    assert!(data.bold && !data.italic);
    assert!((data.font_size - 18.0).abs() < 0.5, "size {}", data.font_size);
    assert_eq!(data.color, "#cc1133");
    // None for a non-existent / non-free-text handle.
    assert!(read_free_text(&out, "nope").expect("read").is_none());
}

/// SPEC: P3-ANN-013 — update_free_text rewrites text + style in place and keeps
/// the same `/NM` (so the sidebar selection / identity survives).
#[test]
fn cos_update_free_text_changes_text_keeps_nm() {
    let bytes = fixture_bytes("hello.pdf");
    let out = add_free_text(&bytes, 0, [100.0, 600.0, 300.0, 640.0], "before", "Helvetica", 14.0, "#000000", false, false)
        .expect("add");
    let nm = read_annotations(&out).expect("read")[0].id.clone();

    let updated = update_free_text(&out, &nm, "after edited", "Times", 20.0, "#ff0000", false, true)
        .expect("update");
    assert_eq!(pdfium_page_count(&updated), 1);

    let data = read_free_text(&updated, &nm).expect("read").expect("some");
    assert_eq!(data.text, "after edited");
    assert_eq!(data.font_family, "Times");
    assert!(data.italic && !data.bold);
    assert!((data.font_size - 20.0).abs() < 0.5);
    assert_eq!(data.color, "#ff0000");

    // The /NM is unchanged: the (only) annotation still has the same handle.
    let ids: Vec<String> = read_annotations(&updated).expect("read").into_iter().map(|a| a.id).collect();
    assert_eq!(ids, vec![nm]);
}

#[test]
fn cos_update_free_text_rejects_unknown_nm() {
    let bytes = add_free_text(&fixture_bytes("hello.pdf"), 0, [10.0, 10.0, 200.0, 60.0], "x", "Helvetica", 12.0, "#000000", false, false).unwrap();
    assert!(update_free_text(&bytes, "no-such-nm", "y", "Helvetica", 12.0, "#000000", false, false).is_err());
}

#[test]
fn cos_free_text_rejects_empty_rect_and_bad_font() {
    let bytes = fixture_bytes("hello.pdf");
    assert!(add_free_text(&bytes, 0, [10.0, 10.0, 10.0, 40.0], "x", "Helvetica", 12.0, "#000000", false, false).is_err());
    assert!(add_free_text(&bytes, 0, [10.0, 10.0, 200.0, 40.0], "x", "Comic Sans", 12.0, "#000000", false, false).is_err());
}

/// The single annotation dict on page 1 + its `/AP /N` stream content (no font
/// lookup — for shapes, which use an `ExtGState` resource, not a `/Font`).
fn first_annot_and_ap(bytes: &[u8]) -> (Dictionary, String) {
    let doc = Document::load_mem(bytes).expect("load");
    let page_id = *doc.get_pages().get(&1).expect("page 1");
    let annot_ref = doc
        .get_dictionary(page_id)
        .unwrap()
        .get(b"Annots")
        .and_then(Object::as_array)
        .unwrap()[0]
        .as_reference()
        .unwrap();
    let annot = doc.get_dictionary(annot_ref).unwrap().clone();
    let n = annot.get(b"AP").and_then(Object::as_dict).unwrap().get(b"N").unwrap().as_reference().unwrap();
    let stream = doc.get_object(n).and_then(Object::as_stream).unwrap();
    (annot, String::from_utf8_lossy(&stream.content).into_owned())
}

/// SPEC: P3-ANN-004 — add_shape writes `/Square` for a rectangle and `/Circle`
/// for an ellipse, each with `/C` + a generated `/AP`, and reopens in PDFium.
#[test]
fn cos_add_shape_writes_square_and_circle() {
    let bytes = fixture_bytes("hello.pdf");

    let sq = add_shape(&bytes, 0, "rectangle", [100.0, 600.0, 300.0, 700.0], "#ff0000", None, 1.0, 2.0)
        .expect("rectangle");
    assert_eq!(pdfium_page_count(&sq), 1);
    let (annot, ap) = first_annot_and_ap(&sq);
    assert_eq!(annot.get(b"Subtype").unwrap().as_name().unwrap(), b"Square");
    assert!(annot.get(b"C").and_then(Object::as_array).is_ok(), "stroke /C present");
    assert!(ap.contains(" re") && ap.contains('S'), "rectangle path: {ap}");

    let ci = add_shape(&bytes, 0, "ellipse", [100.0, 600.0, 300.0, 700.0], "#0000ff", None, 1.0, 2.0)
        .expect("ellipse");
    let (annot, ap) = first_annot_and_ap(&ci);
    assert_eq!(annot.get(b"Subtype").unwrap().as_name().unwrap(), b"Circle");
    assert!(ap.contains(" c\n") && ap.contains('h'), "ellipse Béziers: {ap}");
}

#[test]
fn cos_shape_fill_sets_ic_unfilled_omits_it() {
    let bytes = fixture_bytes("hello.pdf");

    let filled = add_shape(&bytes, 0, "rectangle", [10.0, 10.0, 110.0, 60.0], "#000000", Some("#00ff00"), 1.0, 2.0)
        .expect("filled");
    let (annot, ap) = first_annot_and_ap(&filled);
    assert!(annot.get(b"IC").and_then(Object::as_array).is_ok(), "interior colour /IC present");
    assert!(ap.contains("rg"), "fill colour set: {ap}");
    assert!(ap.contains('B'), "fill+stroke paint op: {ap}");

    let unfilled = add_shape(&bytes, 0, "rectangle", [10.0, 10.0, 110.0, 60.0], "#000000", None, 1.0, 2.0)
        .expect("unfilled");
    let (annot, ap) = first_annot_and_ap(&unfilled);
    assert!(annot.get(b"IC").is_err(), "no /IC when unfilled");
    assert!(!ap.contains(" rg"), "no fill colour op: {ap}");
}

#[test]
fn cos_shape_rejects_bad_kind_and_empty_rect() {
    let bytes = fixture_bytes("hello.pdf");
    assert!(add_shape(&bytes, 0, "triangle", [0.0, 0.0, 10.0, 10.0], "#000000", None, 1.0, 1.0).is_err());
    assert!(add_shape(&bytes, 0, "rectangle", [10.0, 10.0, 10.0, 50.0], "#000000", None, 1.0, 1.0).is_err());
}

/// SPEC: P3-ANN-008 — read_annotations surfaces every supported kind (markup /
/// note / free-text) with its page + contents, in page order.
#[test]
fn cos_reads_all_annotation_kinds() {
    let quads = [[72.0_f32, 700.0, 200.0, 700.0, 72.0, 688.0, 200.0, 688.0]];
    let with_hl = add_text_markup(&fixture_bytes("hello.pdf"), 0, "highlight", &quads, "#ffd400", 1.0)
        .expect("markup");
    let with_note = add_text_note(&with_hl, "n1", 0, 100.0, 600.0, "my note", "Ada").expect("note");
    let all = add_free_text(&with_note, 0, [50.0, 400.0, 250.0, 440.0], "boxed", "Helvetica", 12.0, "#000000", false, false)
        .expect("free text");

    let infos = read_annotations(&all).expect("read");
    let kinds: Vec<&str> = infos.iter().map(|a| a.kind.as_str()).collect();
    assert!(kinds.contains(&"highlight"), "{kinds:?}");
    assert!(kinds.contains(&"note"), "{kinds:?}");
    assert!(kinds.contains(&"freetext"), "{kinds:?}");
    assert!(infos.iter().all(|a| a.page == 0), "all on page 0");

    let note = infos.iter().find(|a| a.kind == "note").unwrap();
    assert_eq!(note.contents, "my note");
    assert_eq!(note.author, "Ada");
    // A note carries /M, which parses to a plausible recent epoch (> 2020).
    assert!(note.modified.is_some_and(|m| m > 1_577_836_800_000), "modified: {:?}", note.modified);

    let ft = infos.iter().find(|a| a.kind == "freetext").unwrap();
    assert_eq!(ft.contents, "boxed");
}

/// SPEC: P3-ANN-012 — every annotation our writers create carries a stable
/// `/NM`, which `read_annotations` returns as the delete handle, and
/// `delete_annotation` removes by it.
#[test]
fn cos_annotations_carry_nm_and_delete_by_it() {
    let quads = [[72.0_f32, 700.0, 200.0, 700.0, 72.0, 688.0, 200.0, 688.0]];
    let a = add_text_markup(&fixture_bytes("hello.pdf"), 0, "highlight", &quads, "#ffd400", 1.0).unwrap();
    let b = add_free_text(&a, 0, [50.0, 400.0, 250.0, 440.0], "x", "Helvetica", 12.0, "#000000", false, false).unwrap();
    let all = add_shape(&b, 0, "rectangle", [60.0, 200.0, 260.0, 300.0], "#000000", None, 1.0, 2.0).unwrap();

    let infos = read_annotations(&all).expect("read");
    assert_eq!(infos.len(), 3);
    // Each handle is a real /NM (a uuid), not an `obj:` fallback.
    assert!(infos.iter().all(|i| !i.id.starts_with("obj:")), "{infos:?}");

    let hl = infos.iter().find(|i| i.kind == "highlight").unwrap().id.clone();
    let after = delete_annotation(&all, &hl).expect("delete");
    let left = read_annotations(&after).expect("re-read");
    assert_eq!(left.len(), 2, "only the highlight is gone");
    assert!(!left.iter().any(|i| i.kind == "highlight"));
    assert_eq!(pdfium_page_count(&after), 1);
}

/// SPEC: P3-ANN-012 — an annotation lacking `/NM` (e.g. authored elsewhere) is
/// surfaced with an `obj:<num> <gen>` handle and deletable by it.
#[test]
fn cos_delete_by_object_id_fallback() {
    let quads = [[72.0_f32, 700.0, 200.0, 700.0, 72.0, 688.0, 200.0, 688.0]];
    let bytes = add_text_markup(&fixture_bytes("hello.pdf"), 0, "highlight", &quads, "#ffd400", 1.0).unwrap();

    // Strip the /NM so read_annotations must fall back to an object-id handle.
    let mut doc = Document::load_mem(&bytes).expect("load");
    let page_id = *doc.get_pages().get(&1).unwrap();
    let annot_ref = doc
        .get_dictionary(page_id)
        .unwrap()
        .get(b"Annots")
        .and_then(Object::as_array)
        .unwrap()[0]
        .as_reference()
        .unwrap();
    doc.get_dictionary_mut(annot_ref).unwrap().remove(b"NM");
    let mut stripped = Vec::new();
    doc.save_to(&mut stripped).expect("save");

    let infos = read_annotations(&stripped).expect("read");
    assert_eq!(infos.len(), 1);
    assert!(infos[0].id.starts_with("obj:"), "expected obj fallback, got {}", infos[0].id);

    let after = delete_annotation(&stripped, &infos[0].id).expect("delete");
    assert!(read_annotations(&after).expect("re-read").is_empty(), "the annotation is gone");
}

#[test]
fn cos_read_annotations_skips_links_and_is_empty_on_plain_pdf() {
    // links.pdf's page-1 /Link is not a surfaced kind.
    assert!(read_annotations(&fixture_bytes("links.pdf")).expect("read").iter().all(|a| a.kind != "link"));
    assert!(read_annotations(&fixture_bytes("hello.pdf")).expect("read").is_empty());
}

/// A PDF number (Integer/Real) as f32, for reading a MediaBox in a test.
fn num(obj: &Object) -> f32 {
    match obj {
        Object::Integer(i) => *i as f32,
        Object::Real(r) => *r,
        _ => panic!("expected a number, got {obj:?}"),
    }
}

/// SPEC: P2-PAGE-010 — resize sets the new MediaBox AND scales content (the
/// page's first content stream becomes a `q … cm` scale matrix), and the result
/// reopens in PDFium. hello.pdf is Letter (612×792); resize to A4.
#[test]
fn cos_resizes_sets_mediabox_and_wraps_content() {
    let bytes = fixture_bytes("hello.pdf");
    let out = resize_pages(&bytes, &[0], 595.28, 841.89, true).expect("resize");

    // Reopens in the *other* engine.
    assert_eq!(pdfium_page_count(&out), 1);

    let doc = Document::load_mem(&out).expect("load");
    let page_id = *doc.get_pages().get(&1).expect("page 1");
    let pd = doc.get_dictionary(page_id).expect("page dict");

    // MediaBox is now A4.
    let mb = pd.get(b"MediaBox").and_then(Object::as_array).expect("MediaBox");
    assert!((num(&mb[2]) - 595.28).abs() < 0.5, "width: {:?}", mb[2]);
    assert!((num(&mb[3]) - 841.89).abs() < 0.5, "height: {:?}", mb[3]);

    // Content was scaled, not just relabeled: the first content stream is our
    // `q <matrix> cm` wrapper (proves the box wasn't merely changed).
    let contents = pd.get(b"Contents").and_then(Object::as_array).expect("Contents array");
    let first = contents.first().expect("a content stream").as_reference().expect("ref");
    let stream = doc.get_object(first).and_then(Object::as_stream).expect("stream");
    let text = String::from_utf8_lossy(&stream.content);
    assert!(text.contains("cm"), "first stream should be the scale matrix, got: {text}");
    assert!(text.contains('q'), "scale wrapper should push graphics state: {text}");
}

#[test]
fn cos_reads_top_level_outline() {
    let titles = read_top_level_outline_titles(&fixture_bytes("bookmarks.pdf")).expect("read outline");
    assert_eq!(as_strs(&titles), vec!["Chapter 1", "Chapter 2", "Chapter 3"]);
}

#[test]
fn cos_adds_top_level_bookmark_reopens_in_pdfium() {
    let input = fixture_bytes("hello.pdf");
    assert!(
        read_top_level_outline_titles(&input).expect("read").is_empty(),
        "hello.pdf starts with no outline"
    );

    let out = add_top_level_bookmark(&input, "Intro", 0).expect("add bookmark");

    // lopdf round-trips its own write...
    assert_eq!(as_strs(&read_top_level_outline_titles(&out).expect("re-read")), vec!["Intro"]);

    // ...and PDFium sees the same outline — the cross-library proof.
    let p = temp_pdf(&out);
    let (doc, _meta) = open_pdf(&p, None).expect("pdfium reopen");
    let root = doc.bookmarks().root();
    assert!(root.is_some(), "PDFium should see the lopdf-written outline");
    assert_eq!(root.and_then(|b| b.title()).as_deref(), Some("Intro"));
    drop(doc);
    let _ = std::fs::remove_file(&p);
}

#[test]
fn cos_renames_form_field_reopens_in_pdfium() {
    let input = fixture_bytes("forms.pdf");
    assert_eq!(as_strs(&read_form_field_names(&input).expect("read names")), vec!["name"]);

    let out = rename_form_fields_with_suffix(&input, "_2").expect("rename");
    assert_eq!(as_strs(&read_form_field_names(&out).expect("re-read names")), vec!["name_2"]);

    // Output is still a valid PDF to PDFium.
    assert_eq!(pdfium_page_count(&out), 1);
}

#[test]
fn cos_transforms_preserve_page_count() {
    let hello = fixture_bytes("hello.pdf");
    let with_bookmark = add_top_level_bookmark(&hello, "X", 0).expect("add");
    assert_eq!(pdfium_page_count(&with_bookmark), pdfium_page_count(&hello));

    let forms = fixture_bytes("forms.pdf");
    let renamed = rename_form_fields_with_suffix(&forms, "_2").expect("rename");
    assert_eq!(pdfium_page_count(&renamed), pdfium_page_count(&forms));
}

#[test]
fn cos_reorders_kids_reopens_in_pdfium() {
    let input = fixture_bytes("bookmarks.pdf"); // 6 pages, flat tree
    // Reverse the page order.
    let out = reorder_pages(&input, &[5, 4, 3, 2, 1, 0]).expect("reorder");
    assert_eq!(pdfium_page_count(&out), 6, "reorder preserves page count + reopens in PDFium");
}

#[test]
fn cos_reorder_rejects_bad_permutation() {
    let input = fixture_bytes("bookmarks.pdf");
    assert!(reorder_pages(&input, &[0, 0, 0, 0, 0, 0]).is_err(), "duplicate indices");
    assert!(reorder_pages(&input, &[0, 1, 2]).is_err(), "wrong length (flat-tree check)");
}

#[test]
fn cos_merges_outlines_and_fields_reopens_in_pdfium() {
    // bookmarks.pdf (6 pp, 3 bookmarks) + forms.pdf (1 pp, field "name").
    let merged = merge_documents(&[fixture_bytes("bookmarks.pdf"), fixture_bytes("forms.pdf")])
        .expect("merge");

    // Outline + field survive in the lopdf output...
    assert_eq!(read_top_level_outline_titles(&merged).expect("outline").len(), 3);
    assert_eq!(as_strs(&read_form_field_names(&merged).expect("fields")), vec!["name"]);

    // ...and the bytes reopen cleanly in PDFium with the combined page count.
    assert_eq!(pdfium_page_count(&merged), 7);
}

#[test]
fn cos_registers_widget_fields() {
    // forms.pdf already lists its field in /AcroForm, so re-registering page 0's
    // widget is a no-op for the name set — assert the field is still present and
    // the output reopens in PDFium.
    let out = register_inserted_form_fields(&fixture_bytes("forms.pdf"), 0, 1).expect("register");
    assert!(as_strs(&read_form_field_names(&out).expect("names")).contains(&"name"));
    assert_eq!(pdfium_page_count(&out), 1);
}

#[test]
fn cos_prunes_dead_link() {
    // A /Link with no /Dest and no /A (what page-import leaves) is a dead link.
    let bytes = strip_dest_via_lopdf(&fixture_bytes("links.pdf"), (10, 0));
    assert_eq!(page_annot_count(&bytes, 0), 1, "link present before prune");
    let pruned = prune_dangling_destinations(bytes);
    assert_eq!(page_annot_count(&pruned, 0), 0, "dead link removed");
}

#[test]
fn cos_keeps_valid_link() {
    // Unmodified links.pdf: the page-1 link targets page 3, which still exists.
    let bytes = fixture_bytes("links.pdf");
    let pruned = prune_dangling_destinations(bytes.clone());
    assert_eq!(pruned, bytes, "a clean document is returned unchanged");
    assert_eq!(page_annot_count(&pruned, 0), 1, "valid link kept");
}

/// Writes a lopdf-produced PDF (a bookmark added to `bookmarks.pdf`) to
/// `/tmp/vibepdf-verify-lopdf.pdf` for an optional manual cross-reader check —
/// confirms lopdf output is valid beyond PDFium. Ignored; run on demand:
///   cargo test --test cos cos_writes_verification_artifact -- --ignored
#[test]
#[ignore = "produces a verification artifact; run on demand"]
fn cos_writes_verification_artifact() {
    let out = add_top_level_bookmark(&fixture_bytes("bookmarks.pdf"), "Appendix", 5)
        .expect("add bookmark");
    let path = PathBuf::from("/tmp/vibepdf-verify-lopdf.pdf");
    std::fs::write(&path, &out).expect("write artifact");
    // 4 top-level bookmarks now (3 original + Appendix), still 6 pages.
    assert_eq!(pdfium_page_count(&out), 6);
    eprintln!("wrote lopdf verification artifact to {}", path.display());
}
