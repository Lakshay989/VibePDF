//! SPEC: P7-OCR-008 — export the tables a document draws as an Excel workbook,
//! one sheet a table, and say so when there are none.
//!
//! Almost all of this step already existed: [`crate::pdf::table_detect`] finds
//! the grids (P7.B1b), [`crate::pdf::page_content`] reads the page, and
//! [`crate::pdf::ooxml`] writes the ZIP container (P7.B1a). What is new is the
//! `SpreadsheetML` and one decision about numbers.
//!
//! **Numbers become numbers only when they are unambiguous.** A spreadsheet of
//! text-formatted figures is a poor deliverable, so `-12`, `3.5` and `0` are
//! written as numeric cells. But `412,000` stays **text**, because a comma is a
//! thousands separator in one locale and a decimal point in another — `1,000`
//! is one thousand in en-US and one in de-DE — and a PDF carries no locale to
//! decide with. Silently turning someone's `1,000` into `1000` corrupts a
//! figure they will then do arithmetic on, which is worse than making them
//! convert a column. See [`classify`].
//!
//! **No tables means no file.** The spec says warn, and a workbook containing
//! either prose or nothing is not worth writing, so the caller is told the
//! count was zero and nothing is created.

use std::fmt::Write as _;

use pdfium_render::prelude::PdfDocument;
use serde::Serialize;

use crate::error::CommandError;
use crate::pdf::export_text::{group_pieces, TextPiece};
use crate::pdf::ooxml::{escape_xml, ZipWriter};
use crate::pdf::page_content::{in_cell, page_content, Placed};
use crate::pdf::table_detect::detect_tables;

/// What an export produced. `tables == 0` is the spec's warning case, and then
/// no file was written.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct XlsxExportSummary {
    pub pages: u32,
    pub tables: u32,
    pub cells: u32,
    pub bytes: u64,
}

/// One sheet: a name and its rows of cell text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sheet {
    pub name: String,
    pub rows: Vec<Vec<String>>,
}

/// How a cell's text is written to the workbook.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CellValue<'a> {
    /// A numeric cell, which Excel will sum.
    Number(f64),
    /// A text cell, written inline.
    Text(&'a str),
}

/// Excel's own limit on a sheet name.
const MAX_SHEET_NAME: usize = 31;

/// SPEC: P7-OCR-008 — decide whether a cell's text is a number.
///
/// Deliberately strict: an optional sign, digits, and at most one decimal
/// point. Anything else — a thousands separator, a percent sign, a currency
/// symbol, an exponent, a space — is text, because in every one of those cases
/// either the value or the reader's intent is ambiguous and a spreadsheet is
/// the wrong place to guess.
#[must_use]
pub fn classify(text: &str) -> CellValue<'_> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return CellValue::Text(trimmed);
    }
    let digits = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
    if digits.is_empty() {
        return CellValue::Text(trimmed);
    }
    let mut seen_dot = false;
    for ch in digits.chars() {
        match ch {
            '0'..='9' => {}
            '.' if !seen_dot => seen_dot = true,
            _ => return CellValue::Text(trimmed),
        }
    }
    if !digits.chars().any(char::is_numeric) {
        return CellValue::Text(trimmed);
    }
    trimmed
        .parse::<f64>()
        .map_or(CellValue::Text(trimmed), |value| {
            // A value Excel cannot hold is better kept as the text it was.
            if value.is_finite() {
                CellValue::Number(value)
            } else {
                CellValue::Text(trimmed)
            }
        })
}

/// A spreadsheet column's letters, from a 0-based index: `A`, `Z`, `AA`, `AZ`,
/// `BA`. Base-26 with no zero digit, which is why this is not just a division.
#[must_use]
pub fn column_letters(index: usize) -> String {
    let mut out = Vec::new();
    let mut n = index;
    loop {
        out.push(b'A' + u8::try_from(n % 26).unwrap_or(0));
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_else(|_| "A".to_owned())
}

/// A sheet name Excel will accept: no `[ ] : * ? / \`, not empty, and at most
/// 31 characters. A name that breaks any of those rules is a corrupt workbook,
/// not a cosmetic problem.
#[must_use]
pub fn sheet_name(page: usize, table: usize) -> String {
    let name = format!("Page {} Table {}", page + 1, table + 1);
    sanitise_sheet_name(&name)
}

fn sanitise_sheet_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| !matches!(c, '[' | ']' | ':' | '*' | '?' | '/' | '\\'))
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        return "Sheet".to_owned();
    }
    trimmed.chars().take(MAX_SHEET_NAME).collect()
}

/// Make every name distinct. Two sheets of the same name is a corrupt workbook,
/// and truncation to 31 characters is a way to create that by accident.
#[must_use]
pub fn unique_names(names: &[String]) -> Vec<String> {
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(names.len());
    for name in names {
        let mut candidate = name.clone();
        let mut suffix = 2;
        while !seen.insert(candidate.to_lowercase()) {
            // Excel compares names case-insensitively, so the set is lowered.
            let tag = format!(" ({suffix})");
            let keep = MAX_SHEET_NAME.saturating_sub(tag.len());
            candidate = format!("{}{tag}", name.chars().take(keep).collect::<String>());
            suffix += 1;
        }
        out.push(candidate);
    }
    out
}

/// `[Content_Types].xml`. Every part needs a type, including one override per
/// worksheet — a missing one is "the file is corrupt" with no further detail.
fn content_types(sheets: usize) -> String {
    let mut out = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>"#,
    );
    for i in 1..=sheets {
        let _ = write!(
            out,
            r#"<Override PartName="/xl/worksheets/sheet{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#
        );
    }
    out.push_str("</Types>");
    out
}

const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#;

/// `xl/workbook.xml` — the list of sheets, in order.
fn workbook_xml(names: &[String]) -> String {
    let mut out = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>"#,
    );
    for (i, name) in names.iter().enumerate() {
        let _ = write!(
            out,
            r#"<sheet name="{}" sheetId="{}" r:id="rId{}"/>"#,
            escape_xml(name),
            i + 1,
            i + 1
        );
    }
    out.push_str("</sheets></workbook>");
    out
}

/// `xl/_rels/workbook.xml.rels` — one relationship per worksheet, matching the
/// `r:id`s in the workbook.
fn workbook_rels(sheets: usize) -> String {
    let mut out = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    );
    for i in 1..=sheets {
        let _ = write!(
            out,
            r#"<Relationship Id="rId{i}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{i}.xml"/>"#
        );
    }
    out.push_str("</Relationships>");
    out
}

/// One worksheet. Text cells are written inline (`t="inlineStr"`), which avoids
/// a shared-strings part and the index that has to agree with it.
fn sheet_xml(sheet: &Sheet) -> (String, u32) {
    let mut out = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>"#,
    );
    let mut cells = 0;
    for (r, row) in sheet.rows.iter().enumerate() {
        let _ = write!(out, r#"<row r="{}">"#, r + 1);
        for (c, text) in row.iter().enumerate() {
            if text.trim().is_empty() {
                continue; // an absent cell is smaller than an empty one
            }
            let reference = format!("{}{}", column_letters(c), r + 1);
            match classify(text) {
                CellValue::Number(value) => {
                    // No `t` attribute at all is the numeric default.
                    let _ = write!(out, r#"<c r="{reference}"><v>{value}</v></c>"#);
                }
                CellValue::Text(value) => {
                    let _ = write!(
                        out,
                        r#"<c r="{reference}" t="inlineStr"><is><t xml:space="preserve">{}</t></is></c>"#,
                        escape_xml(value)
                    );
                }
            }
            cells += 1;
        }
        out.push_str("</row>");
    }
    out.push_str("</sheetData></worksheet>");
    (out, cells)
}

/// SPEC: P7-OCR-008 — assemble the workbook.
///
/// # Errors
/// When a part cannot be compressed or the archive cannot be finished.
pub fn build_xlsx(sheets: &[Sheet]) -> Result<(Vec<u8>, u32), CommandError> {
    let names = unique_names(&sheets.iter().map(|s| s.name.clone()).collect::<Vec<_>>());
    let mut zip = ZipWriter::new();
    zip.add("[Content_Types].xml", content_types(sheets.len()).as_bytes(), true)?;
    zip.add("_rels/.rels", ROOT_RELS.as_bytes(), true)?;
    zip.add("xl/workbook.xml", workbook_xml(&names).as_bytes(), true)?;
    zip.add(
        "xl/_rels/workbook.xml.rels",
        workbook_rels(sheets.len()).as_bytes(),
        true,
    )?;
    let mut cells = 0;
    for (i, sheet) in sheets.iter().enumerate() {
        let (xml, count) = sheet_xml(sheet);
        cells += count;
        zip.add(
            &format!("xl/worksheets/sheet{}.xml", i + 1),
            xml.as_bytes(),
            true,
        )?;
    }
    Ok((zip.finish()?, cells))
}

/// SPEC: P7-OCR-008 — convert the tables in `pages` (0-based, empty meaning all
/// of them) to a workbook at `dest`.
///
/// When no tables are found, **nothing is written** and the summary reports
/// zero, which is the spec's warning case. The PDF is only read.
///
/// # Errors
/// When a page cannot be read, the archive cannot be built, or the write fails.
pub fn export_xlsx(
    doc: &PdfDocument<'_>,
    pages: &[usize],
    dest: &std::path::Path,
) -> Result<XlsxExportSummary, CommandError> {
    let wanted: Vec<i32> = {
        let _guard = crate::pdf::document::pdfium_lock()?;
        let count = doc.pages().len();
        if pages.is_empty() {
            (0..count).collect()
        } else {
            pages
                .iter()
                .map(|p| {
                    i32::try_from(*p)
                        .ok()
                        .filter(|index| *index < count)
                        .ok_or_else(|| CommandError::InvalidInput(format!("page out of range: {p}")))
                })
                .collect::<Result<_, _>>()?
        }
    };

    let mut sheets = Vec::new();
    {
        let _guard = crate::pdf::document::pdfium_lock()?;
        for (position, index) in wanted.iter().enumerate() {
            let (runs, _images, rules) = page_content(doc, *index)?;
            for (ordinal, table) in detect_tables(&rules).iter().enumerate() {
                sheets.push(Sheet {
                    name: sheet_name(position, ordinal),
                    rows: table_rows(table, &runs),
                });
            }
        }
    }

    let mut summary = XlsxExportSummary {
        pages: u32::try_from(wanted.len()).unwrap_or(u32::MAX),
        tables: u32::try_from(sheets.len()).unwrap_or(u32::MAX),
        ..XlsxExportSummary::default()
    };
    if sheets.is_empty() {
        // SPEC: P7-OCR-008's warning case. No file, so there is nothing for the
        // user to open and be puzzled by.
        return Ok(summary);
    }

    let (bytes, cells) = build_xlsx(&sheets)?;
    summary.cells = cells;
    summary.bytes = bytes.len() as u64;
    std::fs::write(dest, &bytes)?;
    Ok(summary)
}

/// One table's cells as rows of text, each cell's own runs in reading order.
fn table_rows(table: &crate::pdf::table_detect::Table, runs: &[Placed]) -> Vec<Vec<String>> {
    let mut rows = vec![vec![String::new(); table.column_count()]; table.row_count()];
    for cell in &table.cells {
        let inside: Vec<&Placed> = runs.iter().filter(|p| in_cell(cell, &p.piece)).collect();
        if inside.is_empty() {
            continue;
        }
        let pieces: Vec<TextPiece> = inside.iter().map(|p| p.piece.clone()).collect();
        // The same ordering as everywhere else, so a two-line cell reads right.
        let mut text = String::new();
        for column in group_pieces(&pieces) {
            for line in column {
                for index in line {
                    let piece = &pieces[index];
                    if !text.is_empty() && !text.ends_with(' ') && !piece.text.starts_with(' ') {
                        text.push(' ');
                    }
                    text.push_str(&piece.text);
                }
            }
        }
        if let Some(row) = rows.get_mut(cell.row) {
            if let Some(slot) = row.get_mut(cell.column) {
                // Trimmed for the same reason as a Word cell: PDFium inserts a
                // geometric space between runs, and a stray one in a cell is a
                // value someone may sort or compare.
                slot.clear();
                slot.push_str(text.trim());
            }
        }
    }
    rows
}

#[allow(clippy::expect_used, clippy::unwrap_used, clippy::float_cmp)]
#[cfg(test)]
mod tests {
    use super::*;

    fn number(text: &str) -> Option<f64> {
        match classify(text) {
            CellValue::Number(v) => Some(v),
            CellValue::Text(_) => None,
        }
    }

    #[test]
    fn plain_numbers_become_numbers() {
        assert_eq!(number("0"), Some(0.0));
        assert_eq!(number("1240"), Some(1240.0));
        assert_eq!(number("-2.5"), Some(-2.5));
        assert_eq!(number("+6"), Some(6.0));
        assert_eq!(number("20.125"), Some(20.125));
        // Surrounding space is a layout artefact, not part of the value.
        assert_eq!(number("  42  "), Some(42.0));
    }

    #[test]
    fn a_grouped_number_stays_text() {
        // The decision this module exists to be careful about: a comma is a
        // thousands separator in one locale and a decimal point in another, so
        // `1,000` is one thousand in en-US and one in de-DE. A PDF carries no
        // locale, and quietly turning someone's 1,000 into 1000 corrupts a
        // figure they will then do arithmetic on.
        assert_eq!(number("412,000"), None);
        assert_eq!(number("1,000"), None);
        assert_eq!(number("1.000,50"), None);
    }

    #[test]
    fn anything_with_a_unit_or_symbol_stays_text() {
        for text in ["+6%", "-2%", "$412", "412 USD", "1e9", "1 240", "N/A", "-", "", "  "] {
            assert_eq!(number(text), None, "{text:?} should not be a number");
        }
    }

    #[test]
    fn column_letters_count_in_base_twenty_six_with_no_zero() {
        assert_eq!(column_letters(0), "A");
        assert_eq!(column_letters(25), "Z");
        // The join is the part that is not plain division: after Z comes AA,
        // not BA.
        assert_eq!(column_letters(26), "AA");
        assert_eq!(column_letters(27), "AB");
        assert_eq!(column_letters(51), "AZ");
        assert_eq!(column_letters(52), "BA");
        assert_eq!(column_letters(701), "ZZ");
        assert_eq!(column_letters(702), "AAA");
    }

    #[test]
    fn sheet_names_say_where_the_table_came_from() {
        assert_eq!(sheet_name(0, 0), "Page 1 Table 1");
        assert_eq!(sheet_name(2, 1), "Page 3 Table 2");
    }

    #[test]
    fn a_sheet_name_excel_would_reject_is_repaired() {
        // These characters are not cosmetic: Excel refuses the workbook.
        assert_eq!(sanitise_sheet_name("Sales/Q1"), "SalesQ1");
        assert_eq!(sanitise_sheet_name("A[1]:B*?\\x"), "A1Bx");
        assert_eq!(sanitise_sheet_name("   "), "Sheet");
        assert_eq!(sanitise_sheet_name(""), "Sheet");
        // 31 characters is the limit, so a long name is cut rather than refused.
        let long = "a".repeat(40);
        assert_eq!(sanitise_sheet_name(&long).chars().count(), 31);
    }

    #[test]
    fn duplicate_sheet_names_are_made_distinct() {
        // Two sheets of the same name is a corrupt workbook, and truncating to
        // 31 characters is a way to create that by accident.
        let names = vec!["Data".to_owned(), "Data".to_owned(), "Data".to_owned()];
        let unique = unique_names(&names);
        assert_eq!(unique, vec!["Data", "Data (2)", "Data (3)"]);

        // Excel compares names case-insensitively, so these collide too.
        let mixed = vec!["Data".to_owned(), "DATA".to_owned()];
        let unique = unique_names(&mixed);
        assert_ne!(unique[0].to_lowercase(), unique[1].to_lowercase());

        // A 31-character name plus a suffix must still fit in 31.
        let long = "b".repeat(31);
        let unique = unique_names(&[long.clone(), long]);
        assert!(unique.iter().all(|n| n.chars().count() <= 31), "{unique:?}");
        assert_ne!(unique[0], unique[1]);
    }

    #[test]
    fn a_numeric_cell_has_no_type_attribute_and_a_text_one_does() {
        let sheet = Sheet {
            name: "S".into(),
            rows: vec![vec!["Region".into(), "1240".into(), "412,000".into()]],
        };
        let (xml, cells) = sheet_xml(&sheet);
        assert_eq!(cells, 3);
        assert!(xml.contains(r#"<c r="A1" t="inlineStr">"#), "text cell mistyped");
        // No `t` attribute at all is SpreadsheetML's numeric default.
        assert!(xml.contains(r#"<c r="B1"><v>1240</v></c>"#), "number not numeric: {xml}");
        assert!(xml.contains(r#"<c r="C1" t="inlineStr">"#), "grouped number was made numeric");
    }

    #[test]
    fn an_empty_cell_is_left_out_rather_than_written_blank() {
        let sheet = Sheet {
            name: "S".into(),
            rows: vec![vec!["a".into(), String::new(), "   ".into(), "b".into()]],
        };
        let (xml, cells) = sheet_xml(&sheet);
        assert_eq!(cells, 2);
        assert!(xml.contains(r#"r="A1""#));
        assert!(xml.contains(r#"r="D1""#), "the fourth column moved");
        assert!(!xml.contains(r#"r="B1""#));
    }

    #[test]
    fn cell_text_is_escaped() {
        let sheet = Sheet {
            name: "S".into(),
            rows: vec![vec!["Revenue & Costs".into(), "a<b".into()]],
        };
        let (xml, _) = sheet_xml(&sheet);
        assert!(xml.contains("Revenue &amp; Costs"));
        assert!(xml.contains("a&lt;b"));
    }

    #[test]
    fn every_sheet_gets_a_content_type_and_a_relationship() {
        // Excel reports "the file is corrupt" for a missing one, and says
        // nothing about which.
        let types = content_types(3);
        let rels = workbook_rels(3);
        for i in 1..=3 {
            assert!(types.contains(&format!("/xl/worksheets/sheet{i}.xml")), "type {i}");
            assert!(rels.contains(&format!(r#"Id="rId{i}""#)), "relationship {i}");
            assert!(rels.contains(&format!("worksheets/sheet{i}.xml")), "target {i}");
        }
    }

    #[test]
    fn the_workbook_names_the_sheets_in_order_with_matching_ids() {
        let names = vec!["First".to_owned(), "Second".to_owned()];
        let xml = workbook_xml(&names);
        let first = xml.find("First").expect("first sheet");
        let second = xml.find("Second").expect("second sheet");
        assert!(first < second, "the sheets are out of order");
        assert!(xml.contains(r#"sheetId="1" r:id="rId1""#));
        assert!(xml.contains(r#"sheetId="2" r:id="rId2""#));
    }
}
