//! SPEC: P7-OCR-004 — "table structures where detectable".
//!
//! The detection geometry is unit-tested as a pure function in
//! `pdf::table_detect`; these tests run it through real PDFs and assert on the
//! `WordprocessingML` that comes out.
//!
//! Two things get as much attention as detection itself. A table's text must
//! appear **exactly once** — a converter that emits the cells and then repeats
//! them as paragraphs has doubled the document. And the three fixtures that
//! contain no table must still produce none, because a false table is worse
//! for a reader than no table.

use std::path::{Path, PathBuf};

use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::export_docx::DocxExportSummary;
use vibepdf_lib::pdf::ooxml::read_entry;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

async fn export(name: &str) -> (String, DocxExportSummary, PathBuf) {
    let dir = std::env::temp_dir().join(format!("vibepdf-tbl-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let dest = dir.join("out.docx");
    let handle = DocumentActorHandle::spawn(None, uuid::Uuid::new_v4(), fixture(name), None)
        .expect("opens");
    let summary = handle.export_docx(vec![], dest.clone()).await.expect("export");
    let bytes = std::fs::read(&dest).expect("the file exists");
    let xml = String::from_utf8(read_entry(&bytes, "word/document.xml").expect("document.xml"))
        .expect("UTF-8");
    (xml, summary, dest)
}

fn cleanup(dest: &Path) {
    if let Some(parent) = dest.parent() {
        let _ = std::fs::remove_dir_all(parent);
    }
}

/// The nth `<w:tbl>…</w:tbl>`, bounded at both ends.
///
/// Splitting on `<w:tbl>` alone returns everything *after* it, trailing
/// paragraphs included — which makes an assertion about a table's contents pass
/// on text that is not in the table at all.
fn table_n(xml: &str, n: usize) -> String {
    xml.split("<w:tbl>")
        .nth(n + 1)
        .and_then(|piece| piece.split("</w:tbl>").next())
        .unwrap_or_else(|| panic!("no table {n} in the document"))
        .to_owned()
}

/// The text of every `w:t` in the document, in order.
fn texts(xml: &str) -> Vec<String> {
    xml.split("<w:t xml:space=\"preserve\">")
        .skip(1)
        .filter_map(|piece| piece.split("</w:t>").next())
        .map(str::to_owned)
        .collect()
}

#[tokio::test]
async fn a_ruled_grid_becomes_a_real_table() {
    // SPEC: P7-OCR-004. The fixture draws a three-column, four-row grid on each
    // of its two pages — page 1 as separate stroked lines, page 2 as one path
    // with many subpaths, because generators do both.
    let (xml, summary, dest) = export("table.pdf").await;
    assert_eq!(summary.tables, 3, "one table per page");
    assert_eq!(xml.matches("<w:tbl>").count(), 3);
    assert_eq!(xml.matches("<w:tr>").count(), 12, "four rows a table");
    assert_eq!(xml.matches("<w:tc>").count(), 48, "four columns a row");
    assert_eq!(xml.matches("<w:gridCol").count(), 12);
    cleanup(&dest);
}

#[tokio::test]
async fn a_single_path_grid_is_found_too() {
    // Page 2 draws the whole grid as one path object, so its bounding box is
    // the size of the table and the rules can only be found by walking its
    // segments — through the object matrix, which they are *not* already in.
    let (xml, _, dest) = export("table.pdf").await;
    let second = table_n(&xml, 1);
    assert_eq!(second.matches("<w:tr>").count(), 4, "the single-path grid lost rows");
    assert!(second.contains("Overseas"), "the single-path grid lost its text");
    assert!(second.contains("412,000"));
    cleanup(&dest);
}

#[tokio::test]
async fn a_grid_drawn_under_a_transform_is_found_where_it_is_drawn() {
    // Page 3 draws its grid at half scale under a `cm`. PDFium reports a
    // segment's point *before* the object matrix is applied while its bounding
    // box is after (measured 2026-09-23), so a detector that trusts the raw
    // points mislocates every rule in a transformed document — and most real
    // documents are transformed.
    let (xml, summary, dest) = export("table.pdf").await;
    assert_eq!(summary.tables, 3, "the transformed grid was missed");
    let third = table_n(&xml, 2);
    assert_eq!(third.matches("<w:tr>").count(), 4);
    // And its text is in its cells, not left behind as paragraphs. This is the
    // assertion that fails when the matrix is ignored: the grid is still found,
    // at coordinates the text is nowhere near, so every cell comes out empty.
    assert!(third.contains("298,500"), "the transformed grid lost its text");
    assert!(third.contains("Overseas"));
    cleanup(&dest);
}

#[tokio::test]
async fn cell_text_lands_in_the_right_cell() {
    let (xml, _, dest) = export("table.pdf").await;
    let table = table_n(&xml, 0);
    let first_row = table.split("<w:tr>").nth(1).expect("a row");
    let header: Vec<String> = texts(first_row.split("</w:tr>").next().unwrap_or(""));
    assert_eq!(header, vec!["Region", "Revenue", "Change", "Units"]);

    let last_row = table.split("<w:tr>").nth(4).expect("the fourth row");
    let values: Vec<String> = texts(last_row.split("</w:tr>").next().unwrap_or(""));
    assert_eq!(values, vec!["Overseas", "77,250", "+31%", "310"]);
    cleanup(&dest);
}

#[tokio::test]
async fn a_tables_text_appears_exactly_once() {
    // The failure that doubles a document: emitting the cells *and* keeping
    // their runs in the paragraph flow.
    let (xml, _, dest) = export("table.pdf").await;
    // Three pages, so each string is expected three times — once per page.
    // Needles that appear *only* inside cells. "Overseas" is deliberately not
    // one of them: it is also a word in the sentence after the table, so it
    // legitimately appears four times and would make this test lie.
    for needle in ["Region", "412,000", "298,500", "+31%"] {
        assert_eq!(
            xml.matches(needle).count(),
            3,
            "{needle} should appear once a page, and does not"
        );
    }
    cleanup(&dest);
}

#[tokio::test]
async fn text_around_the_table_is_still_paragraphs() {
    let (xml, summary, dest) = export("table.pdf").await;
    let outside: String = xml
        .split("<w:tbl>")
        .enumerate()
        .filter_map(|(i, piece)| {
            if i == 0 { Some(piece.to_owned()) } else { piece.split("</w:tbl>").nth(1).map(str::to_owned) }
        })
        .collect();
    assert!(outside.contains("The table below summarises the quarter."));
    assert!(outside.contains("Overseas growth came from two accounts."));
    assert!(summary.paragraphs >= 6, "the surrounding prose was swallowed");
    cleanup(&dest);
}

#[tokio::test]
async fn the_table_sits_between_the_paragraphs_that_surround_it() {
    // Reading order: the sentence introducing the table comes before it, and
    // the one after it comes after.
    let (xml, _, dest) = export("table.pdf").await;
    let intro = xml.find("The table below").expect("intro");
    let table = xml.find("<w:tbl>").expect("table");
    let after = xml.find("Overseas growth").expect("closing line");
    assert!(intro < table, "the table came before its introduction");
    assert!(table < after, "the table came after the line that follows it");
    cleanup(&dest);
}

#[tokio::test]
async fn adding_tables_did_not_disturb_reading_order() {
    // The regression this step actually caused, caught by B1a's own tests.
    //
    // Placing a table among the paragraphs first sorted every block on the page
    // by its top edge — which on a two-column page interleaves the columns
    // again and undoes `group_pieces` entirely. Reading order is not
    // top-to-bottom order, so a table is *inserted into* the paragraph
    // sequence and the sequence is never re-sorted.
    let (xml, summary, dest) = export("two-column.pdf").await;
    assert_eq!(summary.tables, 0);
    let at = |needle: &str| xml.find(needle).unwrap_or_else(|| panic!("missing {needle}"));
    assert!(at("Alpha five") < at("Beta one"), "the columns were interleaved");
    cleanup(&dest);
}

#[tokio::test]
async fn a_section_rule_does_not_become_a_table() {
    // The fixture's decoys: page-wide rules under the heading and above the
    // footer. They have no verticals and bound nothing.
    let (_, summary, dest) = export("table.pdf").await;
    assert_eq!(summary.tables, 3, "a decoy rule was detected as a table");
    cleanup(&dest);
}

#[tokio::test]
async fn documents_without_tables_still_have_none() {
    // The B1a regression guard. A converter that finds tables in ordinary
    // prose has made every document worse, not just table-bearing ones.
    for name in ["report.pdf", "two-column.pdf", "unicode-text.pdf", "hello.pdf"] {
        let (xml, summary, dest) = export(name).await;
        assert_eq!(summary.tables, 0, "{name} produced a table");
        assert!(!xml.contains("<w:tbl>"), "{name} emitted table markup");
        cleanup(&dest);
    }
}

#[tokio::test]
async fn the_archive_still_passes_an_outside_checker() {
    // Table markup is the easiest thing to get structurally wrong — an empty
    // `w:tc` alone is enough for "the file is corrupt".
    let dir = std::env::temp_dir().join(format!("vibepdf-tbl-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let dest = dir.join("out.docx");
    let handle = DocumentActorHandle::spawn(None, uuid::Uuid::new_v4(), fixture("table.pdf"), None)
        .expect("opens");
    handle.export_docx(vec![], dest.clone()).await.expect("export");

    let Ok(output) = std::process::Command::new("unzip").arg("-t").arg(&dest).output() else {
        eprintln!("skipping: `unzip` is not on PATH");
        cleanup(&dest);
        return;
    };
    assert!(output.status.success(), "unzip -t rejected the archive");
    cleanup(&dest);
}
