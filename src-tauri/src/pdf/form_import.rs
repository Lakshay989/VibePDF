//! Form-data import (P5.C2) — FDF / XFDF / JSON / CSV back into the fields.
//!
//! SPEC: P5-FORM-009 — "WHEN the user imports form data, THE system SHALL fill
//! matching fields by name. Unmatched fields SHALL be reported. Type mismatches
//! SHALL be reported, not silently coerced."
//!
//! The exact inverse of [`crate::pdf::form_data`], and it reads the same four
//! formats that module writes — so export → import round-trips. Matching is on
//! the **fully-qualified** field name, which is the handle export addresses
//! fields by.
//!
//! The spec's two "SHALL be reported" clauses are why this returns an
//! [`ImportReport`] rather than a bare count: an import that silently applied
//! what it could and dropped the rest would satisfy the first sentence and
//! violate the other two. A rejected datum is never partially applied — the
//! whole entry is skipped and named in the report.
//!
//! "Type mismatch" covers four cases, all of which a coercing importer would
//! paper over:
//! * the data declares a type the document's field isn't (JSON/CSV/our FDF carry
//!   `type`; XFDF doesn't, so it's validated by value shape alone),
//! * several values for a field that holds one,
//! * a button value that isn't one of that field's appearance states,
//! * a choice value that isn't in the field's `/Opt`.

use std::collections::HashMap;

use lopdf::{Document, Object};
use pdfium_render::prelude::PdfDocument;

use crate::error::CommandError;
use crate::pdf::document::{pdfium, pdfium_lock};
use crate::pdf::form::{
    decode_pdf_text_string, field_kind, field_widget_ids, qualified_name, set_button_field_doc,
    set_choice_field_doc, set_text_field_value_doc, terminal_field_ids, widget_on_state,
    FF_MULTISELECT,
};
use crate::pdf::form_data::{ExportFormat, FormDatum};
use crate::pdf::restore::RestoreDocEdit;
use crate::pdf::undo::Edit;

#[allow(clippy::needless_pass_by_value)]
fn lop(e: lopdf::Error) -> CommandError {
    CommandError::PdfError(format!("form import: {e}"))
}

/// One rejected datum: what the file said the field was, and what it actually is.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeMismatch {
    pub name: String,
    /// What the data claims (its declared type, or the shape of its value).
    pub expected: String,
    /// What the document's field actually is.
    pub got: String,
}

/// SPEC: P5-FORM-009 — the outcome of an import, as the panel reports it.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    /// Fields filled.
    pub applied: usize,
    /// Names present in the data with no matching field in the document.
    pub unmatched: Vec<String>,
    /// Entries whose type disagreed with the field's — reported, not coerced.
    pub mismatched: Vec<TypeMismatch>,
}

/// What the `pdf_import_form_data` command replies with: the report plus the
/// post-import history state, so the frontend can update Undo/Redo in the same
/// round-trip every other write command does.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportOutcome {
    #[serde(flatten)]
    pub report: ImportReport,
    pub history: crate::pdf::undo::HistoryState,
}

/// SPEC: P5-FORM-009 — parse `data` as `format` and fill matching fields in
/// `bytes`. Returns the rewritten document plus the report.
pub fn import_form_data(
    bytes: &[u8],
    data: &[u8],
    format: ExportFormat,
) -> Result<(Vec<u8>, ImportReport), CommandError> {
    let incoming = parse(data, format)?;
    let mut doc = Document::load_mem(bytes).map_err(lop)?;
    let report = apply_data(&mut doc, &incoming)?;

    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| CommandError::PdfError(format!("lopdf save: {e}")))?;
    Ok((out, report))
}

/// SPEC: P5-FORM-009 — import `data` into the live document as one undoable
/// edit, returning the inverse (a pre-import byte snapshot) **and** the report.
///
/// This is the `form_apply` chassis every P5 write shares, opened up: `Edit` can
/// only hand back an inverse, and the spec requires the unmatched and mismatched
/// entries to reach the user, so the caller records the inverse itself.
pub fn import_into<'a>(
    doc: &mut PdfDocument<'a>,
    data: &[u8],
    format: ExportFormat,
) -> Result<(Box<dyn Edit<PdfDocument<'a>>>, ImportReport), CommandError> {
    let pre_bytes = {
        let _guard = pdfium_lock()?;
        doc.save_to_bytes().map_err(CommandError::from)?
    };
    let (new_bytes, report) = import_form_data(&pre_bytes, data, format)?;
    {
        let _guard = pdfium_lock()?;
        *doc = pdfium()?.load_pdf_from_byte_vec(new_bytes, None).map_err(CommandError::from)?;
    }
    Ok((Box::new(RestoreDocEdit { bytes: pre_bytes }), report))
}

// ── matching + applying ─────────────────────────────────────────────────────

/// SPEC: P5-FORM-009 — fill by qualified name; collect the unmatched and the
/// type-mismatched instead of coercing them.
fn apply_data(doc: &mut Document, incoming: &[FormDatum]) -> Result<ImportReport, CommandError> {
    // One pass over the field tree, so a large data file stays linear.
    let by_name: HashMap<String, lopdf::ObjectId> = terminal_field_ids(doc)
        .into_iter()
        .filter_map(|id| {
            let dict = doc.get_dictionary(id).ok()?;
            Some((qualified_name(doc, dict)?, id))
        })
        .collect();

    let mut report = ImportReport::default();
    for datum in incoming {
        let Some(&field) = by_name.get(&datum.name) else {
            report.unmatched.push(datum.name.clone());
            continue;
        };
        let Some(kind) = doc.get_dictionary(field).ok().and_then(|d| field_kind(doc, d)) else {
            report.mismatched.push(TypeMismatch {
                name: datum.name.clone(),
                expected: datum.kind.clone(),
                got: "unknown".to_owned(),
            });
            continue;
        };
        if let Some(problem) = mismatch(doc, field, kind, datum) {
            report.mismatched.push(problem);
            continue;
        }

        match kind {
            "text" => {
                set_text_field_value_doc(doc, field, &datum.value.join("\n"))?;
            }
            "checkbox" | "radio" => {
                let on = datum.value.first().map_or("Off", String::as_str);
                set_button_field_doc(doc, field, on, on != "Off")?;
            }
            "combo" | "list" => set_choice_field_doc(doc, field, &datum.name, &datum.value)?,
            // Push-buttons hold no value and signatures aren't importable data;
            // `mismatch` has already rejected any datum aimed at one.
            _ => continue,
        }
        report.applied += 1;
    }
    Ok(report)
}

/// The type check. `None` ⇒ the datum is safe to apply. Never coerces: every
/// disagreement becomes a [`TypeMismatch`] the caller reports and skips.
fn mismatch(
    doc: &Document,
    field: lopdf::ObjectId,
    kind: &str,
    datum: &FormDatum,
) -> Option<TypeMismatch> {
    let bad = |expected: String| {
        Some(TypeMismatch { name: datum.name.clone(), expected, got: kind.to_owned() })
    };

    // A declared type (JSON / CSV / our FDF) that disagrees outright. XFDF has
    // none, and leaves `kind` empty — then only the value-shape checks apply.
    if !datum.kind.is_empty() && datum.kind != kind {
        return bad(datum.kind.clone());
    }
    // Never importable: no value to carry.
    if matches!(kind, "pushbutton" | "signature") {
        return bad(datum.kind.clone());
    }
    // More than one value only ever fits a multi-select list.
    let multi_ok = kind == "list"
        && doc
            .get_dictionary(field)
            .ok()
            .and_then(|d| crate::pdf::form::inherited(doc, d, b"Ff"))
            .and_then(|o| o.as_i64().ok())
            .unwrap_or(0)
            & FF_MULTISELECT
            != 0;
    if datum.value.len() > 1 && !multi_ok {
        return bad(format!("{} values", datum.value.len()));
    }
    // A button value must name one of the field's own appearance states.
    if matches!(kind, "checkbox" | "radio") {
        if let Some(v) = datum.value.first() {
            let states: Vec<String> = field_widget_ids(doc, field)
                .iter()
                .filter_map(|&w| doc.get_dictionary(w).ok().and_then(|d| widget_on_state(doc, d)))
                .collect();
            if v != "Off" && !states.iter().any(|s| s == v) {
                return bad(format!("state {v}"));
            }
        }
    }
    None
}

// ── parsers ─────────────────────────────────────────────────────────────────

/// SPEC: P5-FORM-009 — decode `data` in `format` into name/type/value entries.
fn parse(data: &[u8], format: ExportFormat) -> Result<Vec<FormDatum>, CommandError> {
    match format {
        ExportFormat::Fdf => from_fdf(data),
        ExportFormat::Xfdf => from_xfdf(&String::from_utf8_lossy(data)),
        ExportFormat::Json => from_json(data),
        ExportFormat::Csv => Ok(from_csv(&String::from_utf8_lossy(data))),
    }
}

/// An FDF file is PDF syntax under an `%FDF-` header, which lopdf's loader
/// rejects on sight — so swap the header back before parsing, the mirror of what
/// [`crate::pdf::form_data::to_fdf`] does on the way out.
fn from_fdf(data: &[u8]) -> Result<Vec<FormDatum>, CommandError> {
    let owned;
    let pdf_bytes: &[u8] = if data.starts_with(b"%FDF-") {
        owned = [b"%PDF-".as_slice(), &data[b"%FDF-".len()..]].concat();
        &owned
    } else if data.starts_with(b"%PDF-") {
        data
    } else {
        return Err(CommandError::InvalidInput("not an FDF file (no %FDF- header)".into()));
    };

    let doc = Document::load_mem(pdf_bytes)
        .map_err(|e| CommandError::InvalidInput(format!("unreadable FDF: {e}")))?;
    let root = doc.trailer.get(b"Root").and_then(Object::as_reference).map_err(lop)?;
    let fdf = doc
        .get_dictionary(root)
        .map_err(lop)?
        .get(b"FDF")
        .ok()
        .and_then(|o| match o {
            Object::Reference(id) => doc.get_dictionary(*id).ok(),
            Object::Dictionary(d) => Some(d),
            _ => None,
        })
        .ok_or_else(|| CommandError::InvalidInput("FDF has no /FDF dictionary".into()))?;
    let fields = fdf.get(b"Fields").and_then(Object::as_array).map_err(lop)?;

    let mut out = Vec::new();
    for entry in fields {
        let Some(d) = (match entry {
            Object::Reference(id) => doc.get_dictionary(*id).ok(),
            Object::Dictionary(d) => Some(d),
            _ => None,
        }) else {
            continue;
        };
        let Ok(name) = d.get(b"T").and_then(Object::as_str) else { continue };
        out.push(FormDatum {
            name: decode_pdf_text_string(name),
            // FDF carries no type; the value shape is the only signal.
            kind: String::new(),
            value: d.get(b"V").map(fdf_values).unwrap_or_default(),
        });
    }
    Ok(out)
}

fn fdf_values(v: &Object) -> Vec<String> {
    match v {
        Object::String(s, _) => vec![decode_pdf_text_string(s)],
        Object::Name(n) => vec![decode_pdf_text_string(n)],
        Object::Array(a) => a.iter().flat_map(fdf_values).collect(),
        _ => Vec::new(),
    }
}

/// Parse XFDF's `<field name="…"><value>…</value></field>`. Hand-rolled, like
/// [`crate::pdf::xfdf`]'s annotation reader — a real XML parser is a dependency
/// we don't need for a shape this narrow. Unknown elements are skipped.
fn from_xfdf(xml: &str) -> Result<Vec<FormDatum>, CommandError> {
    if !xml.contains("<xfdf") {
        return Err(CommandError::InvalidInput("not an XFDF file (no <xfdf> root)".into()));
    }
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<field ") {
        rest = &rest[start..];
        let Some(open_end) = rest.find('>') else { break };
        let Some(name) = attr(&rest[..open_end], "name") else {
            rest = &rest[open_end + 1..];
            continue;
        };
        // The element body ends at its own </field> (fields don't nest here).
        let body_end = rest.find("</field>").unwrap_or(rest.len());
        let body = &rest[open_end + 1..body_end.max(open_end + 1)];

        let mut value = Vec::new();
        let mut scan = body;
        while let Some(vs) = scan.find("<value>") {
            let after = &scan[vs + "<value>".len()..];
            let Some(ve) = after.find("</value>") else { break };
            value.push(xml_unescape(&after[..ve]));
            scan = &after[ve..];
        }
        out.push(FormDatum { name: xml_unescape(&name), kind: String::new(), value });
        rest = &rest[body_end.min(rest.len())..];
        if rest.starts_with("</field>") {
            rest = &rest["</field>".len()..];
        }
    }
    Ok(out)
}

/// The value of `attr` in an element's opening tag, double- or single-quoted.
fn attr(open_tag: &str, attr: &str) -> Option<String> {
    let at = open_tag.find(&format!("{attr}="))? + attr.len() + 1;
    let rest = &open_tag[at..];
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let end = rest[1..].find(quote)?;
    Some(rest[1..=end].to_owned())
}

/// The inverse of `form_data::xml_escape` (the five predefined entities). `&amp;`
/// is decoded last so an escaped `&amp;lt;` doesn't become `<`.
fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn from_json(data: &[u8]) -> Result<Vec<FormDatum>, CommandError> {
    serde_json::from_slice(data)
        .map_err(|e| CommandError::InvalidInput(format!("unreadable JSON form data: {e}")))
}

/// Parse `name,type,value` CSV (RFC 4180 quoting). The header row is optional;
/// multi-values are `;`-joined inside the value cell, matching `to_csv`.
fn from_csv(text: &str) -> Vec<FormDatum> {
    let mut out = Vec::new();
    for row in csv_rows(text) {
        if row.len() < 2 {
            continue;
        }
        if row[0] == "name" && row[1] == "type" {
            continue; // header
        }
        let raw = row.get(2).map_or("", String::as_str);
        let value =
            if raw.is_empty() { Vec::new() } else { raw.split(';').map(str::to_owned).collect() };
        out.push(FormDatum { name: row[0].clone(), kind: row[1].clone(), value });
    }
    out
}

/// Split CSV text into rows of cells, honouring RFC 4180 quoting (a quoted cell
/// may hold commas, newlines, and `""`-escaped quotes).
fn csv_rows(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '"' if quoted => {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cell.push('"');
                } else {
                    quoted = false;
                }
            }
            '"' => quoted = true,
            ',' if !quoted => row.push(std::mem::take(&mut cell)),
            '\r' if !quoted => {} // CRLF: the \n ends the row
            '\n' if !quoted => {
                row.push(std::mem::take(&mut cell));
                rows.push(std::mem::take(&mut row));
            }
            _ => cell.push(c),
        }
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    rows
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::{attr, csv_rows, from_csv, from_xfdf, xml_unescape};

    #[test]
    fn csv_splits_quoted_commas_and_doubled_quotes() {
        let rows = csv_rows("a,\"b,c\",\"say \"\"hi\"\"\"\n");
        assert_eq!(rows, vec![vec!["a", "b,c", "say \"hi\""]]);
    }

    #[test]
    fn csv_keeps_a_quoted_newline_inside_one_cell() {
        let rows = csv_rows("a,\"line1\nline2\"\n");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0][1], "line1\nline2");
    }

    #[test]
    fn csv_skips_the_header_and_splits_multi_values() {
        let data = from_csv("name,type,value\ncolors,list,Red;Blue\n");
        assert_eq!(data.len(), 1);
        assert_eq!(data[0].name, "colors");
        assert_eq!(data[0].kind, "list");
        assert_eq!(data[0].value, vec!["Red", "Blue"]);
    }

    #[test]
    fn csv_empty_value_cell_is_no_values() {
        let data = from_csv("name,type,value\nsig,signature,\n");
        assert!(data[0].value.is_empty());
    }

    #[test]
    fn xfdf_reads_name_and_every_value() {
        let x = "<xfdf><fields><field name=\"colors\"><value>Red</value><value>Blue</value></field>\
                 <field name=\"who\"><value>Ada</value></field></fields></xfdf>";
        let data = from_xfdf(x).unwrap();
        assert_eq!(data.len(), 2);
        assert_eq!(data[0].value, vec!["Red", "Blue"]);
        assert_eq!(data[1].name, "who");
        // XFDF declares no type — the importer validates by value shape only.
        assert!(data[0].kind.is_empty());
    }

    #[test]
    fn xfdf_unescapes_entities() {
        let x = "<xfdf><field name=\"a&amp;b\"><value>x &lt; y</value></field></xfdf>";
        let data = from_xfdf(x).unwrap();
        assert_eq!(data[0].name, "a&b");
        assert_eq!(data[0].value, vec!["x < y"]);
    }

    #[test]
    fn xfdf_rejects_a_non_xfdf_file() {
        assert!(from_xfdf("<html><body/></html>").is_err());
    }

    #[test]
    fn attr_reads_both_quote_styles() {
        assert_eq!(attr("<field name=\"a\"", "name").as_deref(), Some("a"));
        assert_eq!(attr("<field name='b'", "name").as_deref(), Some("b"));
    }

    #[test]
    fn unescape_does_not_double_decode_ampersands() {
        assert_eq!(xml_unescape("&amp;lt;"), "&lt;");
    }
}
