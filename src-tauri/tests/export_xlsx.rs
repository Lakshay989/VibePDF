//! SPEC: P7-OCR-008 — exporting detected tables as an Excel workbook.
//!
//! Every test unzips the output and reads the `SpreadsheetML`. The archive is
//! also checked with `unzip -t` — an implementation we did not write — and the
//! verdict belongs to Excel in the acceptance pass, as with the Word export.
//!
//! The second half of the spec line gets equal weight: when a document has no
//! tables, the caller must be told, and no file is written. A workbook of prose
//! is useless and an empty one is worse.

use std::path::{Path, PathBuf};

use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::export_xlsx::XlsxExportSummary;
use vibepdf_lib::pdf::ooxml::{entry_names, read_entry};

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

async fn export(name: &str) -> (Option<Vec<u8>>, XlsxExportSummary, PathBuf) {
    let dir = std::env::temp_dir().join(format!("vibepdf-xlsx-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let dest = dir.join("out.xlsx");
    let handle = DocumentActorHandle::spawn(None, uuid::Uuid::new_v4(), fixture(name), None)
        .expect("opens");
    let summary = handle.export_xlsx(vec![], dest.clone()).await.expect("export");
    let bytes = std::fs::read(&dest).ok();
    (bytes, summary, dest)
}

fn part(archive: &[u8], name: &str) -> String {
    String::from_utf8(read_entry(archive, name).unwrap_or_else(|_| panic!("missing {name}")))
        .expect("UTF-8")
}

/// The value of `attribute="…"` at or after `from`, and where it ended.
///
/// Hand-walking offsets in a test is how a test comes to assert the wrong
/// thing: the first attempt here used `at + 7` for a six-character marker and
/// compared ids that had lost their first letter.
fn attribute(xml: &str, attribute: &str, from: usize) -> Option<(String, usize)> {
    let marker = format!("{attribute}=\"");
    let at = xml[from..].find(&marker)? + from + marker.len();
    let end = at + xml[at..].find('"')?;
    Some((xml[at..end].to_owned(), end))
}

fn cleanup(dest: &Path) {
    if let Some(parent) = dest.parent() {
        let _ = std::fs::remove_dir_all(parent);
    }
}

#[tokio::test]
async fn every_detected_table_becomes_a_sheet() {
    // SPEC: P7-OCR-008. The fixture draws the same grid on three pages, each a
    // different way, so three sheets is also a check that all three were found.
    let (bytes, summary, dest) = export("table.pdf").await;
    let bytes = bytes.expect("a file was written");
    assert_eq!(summary.tables, 3);
    assert_eq!(summary.cells, 48, "four columns x four rows x three pages");

    let names = entry_names(&bytes).expect("names");
    for required in [
        "[Content_Types].xml",
        "_rels/.rels",
        "xl/workbook.xml",
        "xl/_rels/workbook.xml.rels",
        "xl/worksheets/sheet1.xml",
        "xl/worksheets/sheet2.xml",
        "xl/worksheets/sheet3.xml",
    ] {
        assert!(names.contains(&(*required).to_owned()), "missing {required} in {names:?}");
    }

    let workbook = part(&bytes, "xl/workbook.xml");
    assert!(workbook.contains(r#"name="Page 1 Table 1""#));
    assert!(workbook.contains(r#"name="Page 3 Table 1""#));
    cleanup(&dest);
}

#[tokio::test]
async fn the_cells_hold_the_table_in_the_right_places() {
    let (bytes, _, dest) = export("table.pdf").await;
    let sheet = part(&bytes.expect("a file"), "xl/worksheets/sheet1.xml");
    // The header row.
    assert!(sheet.contains(">Region</t>"), "A1");
    assert!(sheet.contains(">Units</t>"), "D1");
    // A row of data, by reference rather than by order.
    assert!(sheet.contains(r#"<c r="A4" t="inlineStr"><is><t xml:space="preserve">Overseas</t>"#));
    assert!(sheet.contains(r#"<c r="D2"><v>1240</v></c>"#));
    cleanup(&dest);
}

#[tokio::test]
async fn plain_numbers_arrive_as_numbers_and_grouped_ones_as_text() {
    // SPEC: P7-OCR-008 — "extract tables to sheets" means usable data, so a
    // plain integer is a numeric cell. A grouped figure is not, because a comma
    // means different things in different locales and a PDF carries none.
    let (bytes, _, dest) = export("table.pdf").await;
    let sheet = part(&bytes.expect("a file"), "xl/worksheets/sheet1.xml");
    for reference in ["D2", "D3", "D4"] {
        assert!(
            sheet.contains(&format!(r#"<c r="{reference}"><v>"#)),
            "{reference} should be a numeric cell"
        );
    }
    for reference in ["B2", "B3", "B4"] {
        assert!(
            sheet.contains(&format!(r#"<c r="{reference}" t="inlineStr">"#)),
            "{reference} should have stayed text"
        );
    }
    cleanup(&dest);
}

#[tokio::test]
async fn a_document_with_no_tables_writes_no_file_and_says_so() {
    // SPEC: P7-OCR-008's second clause. The caller gets a count of zero and
    // there is no file to open and be puzzled by.
    for name in ["report.pdf", "two-column.pdf", "hello.pdf"] {
        let (bytes, summary, dest) = export(name).await;
        assert_eq!(summary.tables, 0, "{name} reported a table");
        assert_eq!(summary.cells, 0);
        assert_eq!(summary.bytes, 0);
        assert!(bytes.is_none(), "{name} wrote a workbook with nothing in it");
        assert!(!dest.exists(), "{name} left a file behind");
        cleanup(&dest);
    }
}

#[tokio::test]
async fn the_archive_passes_an_outside_checker() {
    let (_, _, dest) = export("table.pdf").await;
    let Ok(output) = std::process::Command::new("unzip").arg("-t").arg(&dest).output() else {
        eprintln!("skipping: `unzip` is not on PATH");
        cleanup(&dest);
        return;
    };
    assert!(
        output.status.success(),
        "unzip -t rejected the workbook:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );
    cleanup(&dest);
}

#[tokio::test]
async fn every_sheet_in_the_workbook_has_a_part_behind_it() {
    // The three-way agreement Excel needs: the workbook names a sheet, a
    // relationship resolves its id, and a part exists at that target. Any one
    // missing is "the file is corrupt".
    let (bytes, _, dest) = export("table.pdf").await;
    let bytes = bytes.expect("a file");
    let workbook = part(&bytes, "xl/workbook.xml");
    let rels = part(&bytes, "xl/_rels/workbook.xml.rels");
    let names = entry_names(&bytes).expect("names");

    let mut ids = Vec::new();
    let mut cursor = 0;
    while let Some((id, end)) = attribute(&workbook, "r:id", cursor) {
        ids.push(id);
        cursor = end;
    }
    assert_eq!(ids.len(), 3, "expected one r:id a sheet, got {ids:?}");

    for id in ids {
        let marker = format!(r#"Id="{id}""#);
        let at = rels.find(&marker).unwrap_or_else(|| panic!("no relationship for {id}"));
        let (target, _) = attribute(&rels, "Target", at).expect("a target");
        let path = format!("xl/{target}");
        assert!(names.contains(&path), "{id} points at missing {path}");
    }
    cleanup(&dest);
}

#[tokio::test]
async fn a_scan_has_no_tables_to_find() {
    // A page that is a picture of a table draws no rules PDFium can see, so
    // there is nothing to detect — a fact about the document, not an error.
    let (bytes, summary, dest) = export("scan.pdf").await;
    assert_eq!(summary.tables, 0);
    assert!(bytes.is_none());
    cleanup(&dest);
}

/// Writes a workbook into `Sample PDFs/verify-xlsx/` for a human to open in
/// Excel — the one verdict nothing here can stand in for.
///
/// Ignored: it writes outside the test scratch directory.
/// `cargo test --test export_xlsx -- --ignored writes_the_verification_file`
#[tokio::test]
#[ignore = "writes a verification file for a human to open"]
async fn writes_the_verification_file() {
    let dir = PathBuf::from("../Sample PDFs/verify-xlsx");
    std::fs::create_dir_all(&dir).expect("verification dir");
    let dest = dir.join("table.xlsx");
    let handle = DocumentActorHandle::spawn(None, uuid::Uuid::new_v4(), fixture("table.pdf"), None)
        .expect("opens");
    let summary = handle.export_xlsx(vec![], dest.clone()).await.expect("export");
    println!(
        "VERIFY {}: {} tables, {} cells, {} bytes",
        dest.display(),
        summary.tables,
        summary.cells,
        summary.bytes
    );
}
