//! Integration tests for "Clean document" (P6.D3).
//!
//! SPEC: P6-SEC-012 — remove metadata, hidden text, comments, attachments,
//! bookmarks, form data, embedded files; each toggle-able.
//!
//! **These tests assert absence of the data, not absence of the key.** The
//! failure this feature invites is deleting `/Info` from the trailer and
//! leaving the dictionary object behind, still holding the author's name, still
//! findable by anything that greps a PDF. A test written as "the `/Info` key is
//! gone" passes against that bug; a test written as "the string `SECRETAUTHOR`
//! does not appear anywhere in the file" does not.
//!
//! So `leaks` below walks every object *and* decompresses every stream *and*
//! scans the raw bytes. The raw scan alone is not enough either: lopdf
//! compresses content streams on save, so a marker hiding in a content stream
//! is invisible to a byte search and would read as a pass.

use std::path::PathBuf;

use lopdf::{Document, Object};
use vibepdf_lib::pdf::clean::{clean_document, CleanOptions, CleanReport};
use vibepdf_lib::pdf::document::open_pdf;

/// Every marker the fixture plants, with the toggle that should remove it.
const MARKERS: &[&str] = &[
    "SECRETTITLE",
    "SECRETAUTHOR",
    "SECRETCREATOR",
    "SECRETPRODUCER",
    "SECRETSUBJECT",
    "SECRETKEYWORD",
    "SECRETCUSTOM",
    "SECRETXMP",
    "SECRETBOOKMARK1",
    "SECRETBOOKMARK2",
    "SECRETFORMVALUE",
    "SECRETATTACHMENT",
    "SECRETCOMMENT",
    "SECRETHIDDEN",
];

/// The one string that must survive every toggle.
const VISIBLE: &str = "VisibleBodyText";

fn fixture() -> Vec<u8> {
    let p = PathBuf::from("../tests/fixtures/basic/metadata.pdf");
    assert!(
        p.is_file(),
        "fixture missing at {} — run tests/fixtures/basic/generate-metadata.py",
        p.display()
    );
    std::fs::read(p).expect("read fixture")
}

fn holds(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

fn scan(obj: &Object, needle: &[u8]) -> bool {
    match obj {
        Object::String(s, _) => holds(s, needle),
        Object::Name(n) => holds(n, needle),
        Object::Array(a) => a.iter().any(|o| scan(o, needle)),
        Object::Dictionary(d) => d
            .iter()
            .any(|(k, v)| holds(k, needle) || scan(v, needle)),
        Object::Stream(s) => {
            let raw = holds(&s.content, needle);
            let decoded = s
                .decompressed_content()
                .map(|c| holds(&c, needle))
                .unwrap_or(false);
            raw || decoded || s.dict.iter().any(|(k, v)| holds(k, needle) || scan(v, needle))
        }
        _ => false,
    }
}

/// Does `needle` survive anywhere in `bytes` — in an object, inside a
/// compressed stream, or loose in the file?
fn leaks(bytes: &[u8], needle: &str) -> bool {
    let n = needle.as_bytes();
    if holds(bytes, n) {
        return true;
    }
    let Ok(doc) = Document::load_mem(bytes) else {
        return false;
    };
    doc.objects.values().any(|o| scan(o, n)) || doc.trailer.iter().any(|(_, v)| scan(v, n))
}

fn clean(opts: CleanOptions) -> (Vec<u8>, CleanReport) {
    clean_document(&fixture(), &opts).expect("clean")
}

fn all_on() -> CleanOptions {
    CleanOptions {
        metadata: true,
        hidden_text: true,
        comments: true,
        attachments: true,
        bookmarks: true,
        form_data: true,
        embedded_files: true,
    }
}

/// A temp file so PDFium can be pointed at the output.
struct TempPdf(PathBuf);

impl TempPdf {
    fn new(bytes: &[u8]) -> Self {
        let p = std::env::temp_dir().join(format!("vibepdf-clean-{}.pdf", uuid::Uuid::new_v4()));
        std::fs::write(&p, bytes).expect("write temp pdf");
        Self(p)
    }
}

impl Drop for TempPdf {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

// The fixture is load-bearing: every test below is "this marker is gone", which
// passes trivially if the marker was never there. This is the guard.
#[test]
fn the_fixture_carries_every_marker() {
    let bytes = fixture();
    for m in MARKERS {
        assert!(leaks(&bytes, m), "fixture is missing {m}");
    }
    assert!(leaks(&bytes, VISIBLE));
}

#[test]
fn nothing_is_removed_when_every_toggle_is_off() {
    let (out, report) = clean(CleanOptions::default());
    assert!(report.is_empty(), "a no-op clean reported {report:?}");
    for m in MARKERS {
        assert!(leaks(&out, m), "{m} disappeared with every toggle off");
    }
}

// SPEC: P6-SEC-012 — "metadata (author, creator, producer, custom keys)".
//
// SECRETAUTHOR is deliberately in *both* /Info and the XMP packet. A cleaner
// that handles only /Info passes every other assertion in this test and fails
// this one, which is the point of duplicating it.
#[test]
fn metadata_clears_both_stores() {
    let opts = CleanOptions {
        metadata: true,
        ..CleanOptions::default()
    };
    let (out, report) = clean(opts);

    for m in [
        "SECRETTITLE",
        "SECRETAUTHOR",
        "SECRETCREATOR",
        "SECRETPRODUCER",
        "SECRETSUBJECT",
        "SECRETKEYWORD",
        "SECRETCUSTOM",
        "SECRETXMP",
    ] {
        assert!(!leaks(&out, m), "{m} survived the metadata clean");
    }
    assert!(report.info_keys >= 7, "counted {} /Info keys", report.info_keys);
    assert_eq!(report.xmp_packets, 1);

    // Untouched by this toggle.
    assert!(leaks(&out, "SECRETCOMMENT"));
    assert!(leaks(&out, VISIBLE));
}

#[test]
fn bookmarks_are_removed() {
    let opts = CleanOptions {
        bookmarks: true,
        ..CleanOptions::default()
    };
    let (out, report) = clean(opts);

    assert!(!leaks(&out, "SECRETBOOKMARK1"));
    assert!(!leaks(&out, "SECRETBOOKMARK2"));
    assert_eq!(report.bookmarks, 2);
    assert!(leaks(&out, VISIBLE));
}

// "Comments" is markup annotations. The form widget and the file attachment are
// each somebody else's toggle, and removing them here would surprise a user who
// asked only to drop the review notes.
#[test]
fn comments_go_but_form_fields_and_attachments_stay() {
    let opts = CleanOptions {
        comments: true,
        ..CleanOptions::default()
    };
    let (out, report) = clean(opts);

    assert!(!leaks(&out, "SECRETCOMMENT"));
    assert_eq!(report.comments, 1);
    assert!(leaks(&out, "SECRETFORMVALUE"), "the form widget was removed");
    assert!(leaks(&out, "SECRETATTACHMENT"), "the attachment was removed");
}

#[test]
fn attachments_and_embedded_files_both_clear_the_payload() {
    // The annotation and the name tree share one file stream, so neither toggle
    // on its own can be assumed to have taken the bytes with it.
    let opts = CleanOptions {
        attachments: true,
        embedded_files: true,
        ..CleanOptions::default()
    };
    let (out, report) = clean(opts);

    assert!(!leaks(&out, "SECRETATTACHMENT"));
    assert_eq!(report.attachments, 1);
    assert_eq!(report.embedded_files, 1);
    assert!(leaks(&out, VISIBLE));
}

// SPEC: P6-SEC-012 says "form data", not "the form". A cleaned document is
// still fillable — the field survives, its value does not.
#[test]
fn form_data_clears_the_value_and_keeps_the_field() {
    let opts = CleanOptions {
        form_data: true,
        ..CleanOptions::default()
    };
    let (out, report) = clean(opts);

    assert!(!leaks(&out, "SECRETFORMVALUE"));
    assert_eq!(report.form_fields, 1);

    let doc = Document::load_mem(&out).expect("load");
    let root = doc
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .expect("/Root");
    let catalog = doc.get_object(root).and_then(Object::as_dict).expect("catalog");
    assert!(catalog.has(b"AcroForm"), "the form itself was removed");
    assert!(leaks(&out, "secret_field"), "the field name was removed");
}

// Invisible text is real text: no reader draws it, every extractor finds it.
#[test]
fn hidden_text_goes_and_visible_text_stays() {
    let opts = CleanOptions {
        hidden_text: true,
        ..CleanOptions::default()
    };
    let (out, report) = clean(opts);

    assert_eq!(report.hidden_text_runs, 1);
    assert!(!leaks(&out, "SECRETHIDDEN"));

    // …and the page still says what it said. Checked through PDFium's
    // extractor, not our own byte scan, because that is what a reader sees.
    let file = TempPdf::new(&out);
    let (doc, _) = open_pdf(&file.0, None).expect("PDFium opens the cleaned file");
    let runs = vibepdf_lib::pdf::text_extract::extract_text_runs(&doc, 0).expect("extract");
    let text: String = runs.iter().map(|r| r.text.as_str()).collect();
    assert!(text.contains(VISIBLE), "visible text was lost, got {text:?}");
    assert!(!text.contains("SECRETHIDDEN"), "extractor still finds it");
    drop(doc);
}

// The whole point of the feature, and the round-trip rule: a cleaned document
// still opens, still renders, and carries none of what it was asked to drop.
#[test]
fn cleaning_everything_leaves_a_readable_page_and_nothing_else() {
    let (out, report) = clean(all_on());

    for m in MARKERS {
        assert!(!leaks(&out, m), "{m} survived a full clean");
    }
    assert!(!report.is_empty());

    let file = TempPdf::new(&out);
    let (doc, meta) = open_pdf(&file.0, None).expect("PDFium opens the cleaned file");
    assert_eq!(meta.page_count, 1);
    let runs = vibepdf_lib::pdf::text_extract::extract_text_runs(&doc, 0).expect("extract");
    let text: String = runs.iter().map(|r| r.text.as_str()).collect();
    assert!(text.contains(VISIBLE), "the page lost its text, got {text:?}");
    drop(doc);
}

#[test]
fn a_document_with_nothing_to_clean_is_handled() {
    // hello.pdf has no metadata, no form, no outline — every toggle on should
    // be a well-formed no-op rather than an error or a broken file.
    let src = std::fs::read("../tests/fixtures/basic/hello.pdf").expect("hello.pdf");
    let (out, report) = clean_document(&src, &all_on()).expect("clean");
    assert!(report.is_empty(), "reported {report:?} on a bare document");

    let file = TempPdf::new(&out);
    let (doc, meta) = open_pdf(&file.0, None).expect("still opens");
    assert_eq!(meta.page_count, 1);
    drop(doc);
}

/// SPEC: P6-SEC-012 — a file to open in Acrobat / Preview / a third reader.
#[test]
#[ignore = "produces a verification artifact; run on demand"]
fn writes_verification_artifact() {
    let dir = PathBuf::from("../Sample PDFs");
    std::fs::create_dir_all(&dir).expect("ensure Sample PDFs dir");

    for (name, opts) in [
        ("vibepdf-verify-dirty.pdf", CleanOptions::default()),
        ("vibepdf-verify-cleaned.pdf", all_on()),
    ] {
        let (out, _) = clean_document(&fixture(), &opts).expect("clean");
        let path = dir.join(name);
        std::fs::write(&path, &out).expect("write");
        eprintln!("wrote {}", path.display());
    }
}
