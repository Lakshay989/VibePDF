//! Capability spike for the `lopdf` adoption (COS / object-model layer).
//!
//! Proves the three structural operations PDFium's API can't do — outline
//! read, outline write, form-field rename — and, crucially, that every lopdf
//! output **reopens cleanly in PDFium** (the cross-library byte-compatibility
//! guarantee the integration model depends on). No feature is wired to this
//! yet; it validates the dependency choice. Single-threaded (PDFium).

use std::path::PathBuf;

use lopdf::{Document, Object};
use vibepdf_lib::pdf::cos::{
    add_top_level_bookmark, merge_documents, prune_dangling_destinations, read_form_field_names,
    register_inserted_form_fields, read_top_level_outline_titles, rename_form_fields_with_suffix,
    reorder_pages, resize_pages,
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
