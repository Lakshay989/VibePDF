//! True redaction (P6.D1a, SPEC P6-SEC-010).
//!
//! The acceptance from `docs/05_ROADMAP.md` is "redact an SSN, then extract the
//! text and confirm the SSN is not there". These tests do that, and then keep
//! going, because "the string is absent" is satisfied by a great many wrong
//! implementations — including one that deletes the whole page.
//!
//! So each test names both halves: what must be **gone**, and what must have
//! **survived**. A redaction that removes too much is a bug too; it is just a
//! less dangerous one, and without the surviving half a test cannot tell the
//! difference.

use std::path::PathBuf;

use vibepdf_lib::pdf::document::open_pdf;
use vibepdf_lib::security::redact::redact_text_in_region;

const SSN: &str = "123-45-6789";
const SURVIVES: &str = "Employee record";
const IN_A_FORM: &str = "987-65-4321";

/// The region over the SSN digits on page 1. "SSN: " is 31.3pt wide at 12pt in
/// Helvetica, so the number runs from x=103.3 to x=171.4 — see the fixture
/// generator, which lays this out on purpose.
const OVER_THE_NUMBER: [f32; 4] = [102.0, 695.0, 175.0, 713.0];

fn fixture() -> Vec<u8> {
    let p = PathBuf::from("../tests/fixtures/acceptance/p6-document.pdf");
    assert!(
        p.is_file(),
        "fixture missing at {} — run tests/fixtures/acceptance/generate-p6-document.py",
        p.display()
    );
    std::fs::read(p).expect("read fixture")
}

struct TempPdf(PathBuf);

impl TempPdf {
    fn new(bytes: &[u8]) -> Self {
        let p = std::env::temp_dir().join(format!("vibepdf-redact-{}.pdf", uuid::Uuid::new_v4()));
        std::fs::write(&p, bytes).expect("write temp pdf");
        Self(p)
    }
}

impl Drop for TempPdf {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// The text `PDFium` can see on `page` of `bytes` — the same extractor a reader
/// would use, not our own parse. Checking our work with the code that did it
/// proves nothing.
fn extracted(bytes: &[u8], page: usize) -> String {
    let file = TempPdf::new(bytes);
    let (doc, _) = open_pdf(&file.0, None).expect("PDFium opens the redacted file");
    let runs = vibepdf_lib::pdf::text_extract::extract_text_runs(&doc, page).expect("extract");
    let text: String = runs.iter().map(|r| r.text.as_str()).collect();
    drop(doc);
    text
}

/// Does `needle` survive anywhere in the raw bytes, including inside a
/// compressed stream? Extraction alone would miss text a reader does not draw.
fn leaks(bytes: &[u8], needle: &str) -> bool {
    let n = needle.as_bytes();
    if bytes.windows(n.len()).any(|w| w == n) {
        return true;
    }
    let Ok(doc) = lopdf::Document::load_mem(bytes) else {
        return false;
    };
    doc.objects.values().any(|o| {
        o.as_stream()
            .ok()
            .and_then(|s| s.decompressed_content().ok())
            .is_some_and(|c| c.windows(n.len()).any(|w| w == n))
    })
}

// SPEC: P6-SEC-010 — the roadmap's acceptance, both ways round.
#[test]
fn the_ssn_is_gone_from_the_saved_file() {
    let (out, report) = redact_text_in_region(&fixture(), 0, OVER_THE_NUMBER).expect("redact");

    assert!(!leaks(&out, SSN), "the SSN is still in the file's bytes");
    let text = extracted(&out, 0);
    assert!(!text.contains(SSN), "PDFium can still read the SSN: {text:?}");
    assert!(
        text.contains(SURVIVES),
        "the rest of the page was destroyed: {text:?}"
    );
    assert_eq!(report.split, 1, "the run should have been cut, not dropped");
}

// The point of measuring rather than dropping: "SSN:" is not secret and should
// still be on the page, so a reader can see *that* something was removed.
#[test]
fn a_partly_covered_run_keeps_its_visible_half() {
    let (out, _) = redact_text_in_region(&fixture(), 0, OVER_THE_NUMBER).expect("redact");
    let text = extracted(&out, 0);

    assert!(text.contains("SSN"), "the label went with the number: {text:?}");
    assert!(!text.contains(SSN));
}

#[test]
fn text_outside_the_region_survives() {
    let (out, _) = redact_text_in_region(&fixture(), 0, OVER_THE_NUMBER).expect("redact");
    let text = extracted(&out, 0);

    assert!(text.contains(SURVIVES));
    assert!(
        text.contains("Department"),
        "a line well below the region was removed: {text:?}"
    );
}

// The whole distinction the spec draws in clause (a). If the black box were
// doing the work, removing it would bring the SSN back.
#[test]
fn the_black_box_is_not_the_mechanism() {
    let (out, _) = redact_text_in_region(&fixture(), 0, OVER_THE_NUMBER).expect("redact");

    // Strip every fill-rectangle from the page content and look again.
    let mut doc = lopdf::Document::load_mem(&out).expect("load");
    let page_id = *doc.get_pages().values().next().expect("a page");
    let content = doc.get_page_content(page_id).expect("content");
    let mut parsed = lopdf::content::Content::decode(&content).expect("decode");
    parsed
        .operations
        .retain(|op| !matches!(op.operator.as_str(), "re" | "f" | "rg"));
    let stripped = parsed.encode().expect("encode");
    doc.change_page_content(page_id, stripped).expect("replace");

    let mut naked = Vec::new();
    doc.save_to(&mut naked).expect("save");

    assert!(
        !leaks(&naked, SSN),
        "the SSN came back once the box was removed — it was only ever covered"
    );
}

// SPEC: P6-SEC-010 — text inside a Form XObject is invisible to a page-content
// walk. Redacting the page and reporting success would leave it in the file and
// tell the user otherwise, which is the worst thing this module could do.
#[test]
fn a_page_whose_text_hides_in_a_form_is_refused() {
    let err = redact_text_in_region(&fixture(), 1, [0.0, 0.0, 612.0, 792.0]).unwrap_err();
    let msg = format!("{err:?}");

    assert!(msg.contains("form"), "the refusal should say why: {msg}");
    assert!(
        msg.contains("Flatten"),
        "the refusal should say what to do about it: {msg}"
    );
    // …and the text really is there, so the refusal is not hypothetical.
    assert!(leaks(&fixture(), IN_A_FORM));
}

// A region that touches nothing should not rewrite the document. Re-encoding a
// file and calling it a redaction is the kind of no-op that looks like work.
#[test]
fn a_region_over_empty_space_changes_nothing() {
    let source = fixture();
    let (out, report) = redact_text_in_region(&source, 0, [400.0, 100.0, 500.0, 150.0])
        .expect("redact");

    assert!(report.is_empty(), "reported {report:?} for an empty region");
    assert_eq!(out, source, "the document was rewritten for nothing");
}

// The whole line, not just the number.
#[test]
fn a_region_over_a_whole_run_removes_it() {
    let (out, report) = redact_text_in_region(&fixture(), 0, [60.0, 695.0, 400.0, 713.0])
        .expect("redact");

    let text = extracted(&out, 0);
    assert!(!text.contains("SSN"), "the whole line should be gone: {text:?}");
    assert!(text.contains(SURVIVES));
    assert_eq!(report.removed, 1);
    assert_eq!(report.split, 0);
}

// SPEC: P6-SEC-010(c) — the verification helper has to fail when it should, or
// it is decoration. Both directions: a leak, and a check that passed only
// because extraction returned nothing.
#[test]
fn the_verification_helper_catches_both_kinds_of_wrong() {
    use vibepdf_lib::security::redact::confirm_removed;

    assert!(confirm_removed("Employee record", &[SSN], &[SURVIVES]).is_ok());

    let leaked = confirm_removed("Employee record 123-45-6789", &[SSN], &[SURVIVES]);
    assert!(leaked.is_err(), "a leak was not caught");

    // Extraction returned nothing: the SSN is "absent", and so is everything.
    let vacuous = confirm_removed("", &[SSN], &[SURVIVES]);
    assert!(
        vacuous.is_err(),
        "an empty extraction passed — the check proves nothing"
    );
}

/// SPEC: P6-SEC-010 — a redacted file to inspect.
#[test]
#[ignore = "produces a verification artifact; run on demand"]
fn writes_verification_artifact() {
    let dir = PathBuf::from("../Sample PDFs");
    std::fs::create_dir_all(&dir).expect("ensure Sample PDFs dir");
    let (out, _) = redact_text_in_region(&fixture(), 0, OVER_THE_NUMBER).expect("redact");
    let path = dir.join("vibepdf-verify-redacted.pdf");
    std::fs::write(&path, &out).expect("write");
    eprintln!("wrote {}", path.display());
}

// The module's governing principle, exercised rather than merely documented.
//
// Page 3's font has an unrecognised /BaseFont and no /Widths, so its advances
// are unknowable. A region touching *part* of the run must therefore take the
// whole run: guessing the cut point could leave half the account number, which
// is the failure this whole design exists to avoid.
#[test]
fn an_unmeasurable_run_is_removed_whole_and_reported() {
    let source = fixture();
    // Covers the *tail* of "Account: 555-0100", from mid-run to past its end.
    //
    // The region matters. Over the run's start, an implementation that wrongly
    // measured this font would also end up removing everything — the surviving
    // tail is not a prefix, so it cannot be split either way, and the test
    // would pass for the wrong reason. Over the tail, a measuring
    // implementation *can* split (keeping "Account: "), so the two paths
    // diverge and the assertions below can tell them apart.
    let (out, report) = redact_text_in_region(&source, 2, [115.0, 695.0, 400.0, 713.0])
        .expect("redact");

    assert_eq!(
        report.removed_whole_for_safety, 1,
        "the safe default did not fire: {report:?}"
    );
    assert_eq!(report.split, 0, "an unmeasurable run must never be cut");
    assert!(
        !leaks(&out, "555-0100"),
        "part of the account number survived the cut"
    );
    // Over-removal is the price, and it is bounded: the other line is untouched.
    let text = extracted(&out, 2);
    assert!(
        text.contains("Elsewhere on the page"),
        "over-removal spread beyond the run: {text:?}"
    );
}
