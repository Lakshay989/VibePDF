//! Capability spike for the `lopdf` adoption (COS / object-model layer).
//!
//! Proves the three structural operations PDFium's API can't do — outline
//! read, outline write, form-field rename — and, crucially, that every lopdf
//! output **reopens cleanly in PDFium** (the cross-library byte-compatibility
//! guarantee the integration model depends on). No feature is wired to this
//! yet; it validates the dependency choice. Single-threaded (PDFium).

use std::path::PathBuf;

use vibepdf_lib::pdf::cos::{
    add_top_level_bookmark, merge_documents, read_form_field_names, register_inserted_form_fields,
    read_top_level_outline_titles, rename_form_fields_with_suffix, reorder_pages,
};
use vibepdf_lib::pdf::document::open_pdf;

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
