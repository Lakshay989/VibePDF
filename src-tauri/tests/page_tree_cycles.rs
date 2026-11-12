//! SPEC: P1-VIEW-013 — a page tree whose `/Parent` pointers loop must not
//! freeze the app (P7.HF3).
//!
//! `MediaBox`, `CropBox`, `Rotate` and `Resources` can be inherited: when a
//! page has none of its own, the reader climbs `/Parent` until one turns up.
//! `/Parent` is the file's own pointer, and before this step the four walks
//! that climb it (three in `cos.rs`, shared by every page decoration, and one
//! in `background.rs`) did not stop if it led back where it started. The
//! decoration then never finished, on the document's actor thread.
//!
//! These go through the features themselves; the walks' own unit tests are
//! beside them in `cos.rs` and `background.rs`. A regression shows up as a
//! test that never finishes, not as a failed assertion.

use std::path::{Path, PathBuf};

use lopdf::{Document, Object, ObjectId};
use vibepdf_lib::pdf::background::{add_background, BackgroundKind};
use vibepdf_lib::pdf::document::open_pdf;
use vibepdf_lib::pdf::watermark::{add_watermark, WatermarkKind};

// Object numbers from `tests/fixtures/basic/generate-bookmarks.py`.
const PAGES: ObjectId = (2, 0);
const PAGE_1: ObjectId = (3, 0);

fn fixture(name: &str) -> Vec<u8> {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    std::fs::read(&p).unwrap_or_else(|e| panic!("read fixture {}: {e}", p.display()))
}

/// `bookmarks.pdf` (six pages, none with a `/Rotate` or `/CropBox` of its
/// own) after `edit`, re-serialized by lopdf with its object numbers kept.
fn edited(edit: impl FnOnce(&mut Document)) -> Vec<u8> {
    let mut doc = Document::load_mem(&fixture("bookmarks.pdf")).expect("load fixture");
    edit(&mut doc);
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("save");
    out
}

fn draft() -> WatermarkKind {
    WatermarkKind::Text { text: "DRAFT".to_owned(), font_family: "Helvetica".to_owned(), size: 64.0, color: "#808080".to_owned(), bold: false, italic: false }
}

/// The decoded content of page `page_no` (1-based).
fn page_content(bytes: &[u8], page_no: u32) -> String {
    let doc = Document::load_mem(bytes).expect("load");
    let page_id = *doc.get_pages().get(&page_no).expect("page exists");
    String::from_utf8_lossy(&doc.get_page_content(page_id).expect("content")).into_owned()
}

/// The page count `PDFium`, a reader we did not write, gives the result.
fn pdfium_page_count(bytes: &[u8]) -> u32 {
    let path: &Path = &std::env::temp_dir().join(format!("vibepdf-page-tree-cycles-{}.pdf", uuid::Uuid::new_v4()));
    std::fs::write(path, bytes).expect("write");
    let (doc, meta) = open_pdf(path, None).expect("PDFium opens it");
    drop(doc);
    let _ = std::fs::remove_file(path);
    meta.page_count
}

// SPEC: P4-EDIT-009. The page has no `/Rotate`, so the watermark's placement
// climbs `/Parent` for one, and the page's parent is the page.
#[test]
fn a_watermark_on_a_page_that_is_its_own_parent_is_still_drawn() {
    let bytes = edited(|doc| doc.get_dictionary_mut(PAGE_1).expect("page").set("Parent", Object::Reference(PAGE_1)));
    let out = add_watermark(&bytes, &[0], &draft(), 0.3, 45.0, true).expect("watermark");
    assert!(page_content(&out, 1).contains("(DRAFT) Tj"), "page 1 has no watermark");
    assert!(!page_content(&out, 2).contains("(DRAFT) Tj"), "page 2 was not asked for");
    assert_eq!(pdfium_page_count(&out), 6);
}

// SPEC: P4-EDIT-009. The loop is above the pages here: the `/Pages` node
// names itself as its parent, so every page's climb runs into it.
#[test]
fn a_watermark_under_a_pages_node_that_is_its_own_parent_is_still_drawn() {
    let bytes = edited(|doc| doc.get_dictionary_mut(PAGES).expect("pages").set("Parent", Object::Reference(PAGES)));
    let out = add_watermark(&bytes, &[0, 5], &draft(), 0.3, 45.0, true).expect("watermark");
    assert!(page_content(&out, 1).contains("(DRAFT) Tj"), "page 1 has no watermark");
    assert!(page_content(&out, 6).contains("(DRAFT) Tj"), "page 6 has no watermark");
    assert_eq!(pdfium_page_count(&out), 6);
}

// SPEC: P4-EDIT-008. The page used as a background comes from a second file
// the user picked. It has no `/Resources` of its own and is its own parent.
#[test]
fn a_background_from_a_page_that_is_its_own_parent_is_still_placed() {
    let source = edited(|doc| {
        let page = doc.get_dictionary_mut(PAGE_1).expect("page");
        page.remove(b"Resources");
        page.set("Parent", Object::Reference(PAGE_1));
    });
    let out = add_background(&fixture("hello.pdf"), &[0], &BackgroundKind::PdfPage { source, page: 0 }, 1.0).expect("background");
    let content = page_content(&out, 1);
    let (form, own) = (content.find("Bgpdf").expect("the background is drawn"), content.find("Hello").expect("the page's own text"));
    assert!(form < own, "the background is not behind the page's own text");
    assert_eq!(pdfium_page_count(&out), 1);
}

/// `Sample PDFs/scratch/page-own-parent.pdf` — the file a person tries in the
/// app (steps/P7-VERIFY.md §1.12): `bookmarks.pdf` with page 1 as its own
/// parent.
/// `node scripts/cargo-test.mjs --test page_tree_cycles writes_the_verification -- --ignored`
#[test]
#[ignore = "writes a verification file for a human to open"]
fn writes_the_verification_file() {
    let dir = PathBuf::from("../Sample PDFs/scratch");
    std::fs::create_dir_all(&dir).expect("dir");
    let bytes = edited(|doc| doc.get_dictionary_mut(PAGE_1).expect("page").set("Parent", Object::Reference(PAGE_1)));
    std::fs::write(dir.join("page-own-parent.pdf"), bytes).expect("write");
}
