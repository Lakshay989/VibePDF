//! Form-data export (P5.C1) — FDF / XFDF / JSON / CSV.
//!
//! SPEC: P5-FORM-008 — "WHEN the user exports form data, THE system SHALL support
//! FDF, XFDF, JSON, and CSV formats. Export SHALL include field name, value, and
//! type."
//!
//! Unlike the fill readers (A2–A4), which are per-page and per-kind, export is
//! **document-wide and kind-agnostic**: it walks the `AcroForm` `/Fields` tree
//! (not each page's `/Annots`), so every field is included exactly once, in form
//! order, addressed by its fully-qualified name — the same handle import (C2)
//! will match on.

use std::fmt::Write as _;

use lopdf::{dictionary, Document, Object};

use crate::error::CommandError;
use crate::pdf::form::{decode_pdf_text_string, field_kind, inherited, qualified_name};

/// One exported field: its qualified name, kind, and value(s). Multi-select list
/// boxes carry several values; a signature carries none.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct FormDatum {
    pub name: String,
    /// `"text" | "checkbox" | "radio" | "combo" | "list" | "signature"`.
    /// Push-buttons are excluded entirely — they hold no value.
    #[serde(rename = "type")]
    pub kind: String,
    pub value: Vec<String>,
}

/// The export formats. Parsed from the wire string by [`ExportFormat::parse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Fdf,
    Xfdf,
    Json,
    Csv,
}

impl ExportFormat {
    /// Parse a wire format name (`"fdf" | "xfdf" | "json" | "csv"`).
    pub fn parse(s: &str) -> Result<Self, CommandError> {
        match s {
            "fdf" => Ok(Self::Fdf),
            "xfdf" => Ok(Self::Xfdf),
            "json" => Ok(Self::Json),
            "csv" => Ok(Self::Csv),
            other => Err(CommandError::InvalidInput(format!("unknown export format: {other}"))),
        }
    }
}

/// A field's value(s): a text string, a Name (buttons), or an array (multi-select).
fn field_values(doc: &Document, dict: &lopdf::Dictionary) -> Vec<String> {
    match inherited(doc, dict, b"V") {
        Some(Object::String(s, _)) => vec![decode_pdf_text_string(s)],
        Some(Object::Name(n)) => vec![decode_pdf_text_string(n)],
        Some(Object::Array(a)) => a
            .iter()
            .filter_map(|e| match e {
                Object::String(s, _) => Some(decode_pdf_text_string(s)),
                Object::Name(n) => Some(decode_pdf_text_string(n)),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// SPEC: P5-FORM-008 — collect every value-carrying field in the document, in
/// `AcroForm` `/Fields` order (terminal fields only; push-buttons excluded).
pub fn collect_form_data(doc: &Document) -> Result<Vec<FormDatum>, CommandError> {
    let Some(acro) = crate::pdf::cos::acroform_dict(doc)? else {
        return Ok(Vec::new());
    };
    let Some(fields) = acro.get(b"Fields").ok().and_then(|o| o.as_array().ok()) else {
        return Ok(Vec::new());
    };

    let mut out = Vec::new();
    let mut visited: std::collections::HashSet<lopdf::ObjectId> = std::collections::HashSet::new();
    // Depth-first over the field tree, preserving /Fields order (push in reverse).
    let mut stack: Vec<Object> = fields.iter().rev().cloned().collect();
    let mut budget = 100_000u32;

    while let Some(obj) = stack.pop() {
        budget = budget.saturating_sub(1);
        if budget == 0 {
            break;
        }
        if let Object::Reference(id) = obj {
            if !visited.insert(id) {
                continue;
            }
        }
        let dict = match &obj {
            Object::Reference(id) => match doc.get_dictionary(*id) {
                Ok(d) => d,
                Err(_) => continue,
            },
            Object::Dictionary(d) => d,
            _ => continue,
        };
        // Container field (kids are themselves fields) → descend, don't emit.
        let field_kids: Vec<Object> = dict
            .get(b"Kids")
            .ok()
            .and_then(|o| o.as_array().ok())
            .map(|kids| {
                kids.iter()
                    .filter(|k| {
                        let kd = match k {
                            Object::Reference(id) => doc.get_dictionary(*id).ok(),
                            Object::Dictionary(d) => Some(d),
                            _ => None,
                        };
                        kd.is_some_and(|d| d.get(b"FT").is_ok() || d.get(b"T").is_ok())
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        if !field_kids.is_empty() {
            stack.extend(field_kids.into_iter().rev());
            continue;
        }
        let Some(kind) = field_kind(doc, dict) else { continue };
        if kind == "pushbutton" {
            continue; // carries no value
        }
        let Some(name) = qualified_name(doc, dict) else { continue };
        out.push(FormDatum { name, kind: kind.to_owned(), value: field_values(doc, dict) });
    }
    Ok(out)
}

/// SPEC: P5-FORM-008 — serialise as **FDF**: a real PDF-syntax file whose catalog
/// carries `/FDF << /Fields [ << /T (name) /V (value) >> … ] >>`. Built with lopdf
/// (not string concatenation) so the output parses as PDF.
pub fn to_fdf(data: &[FormDatum]) -> Result<Vec<u8>, CommandError> {
    let mut doc = Document::with_version("1.2");
    let fields: Vec<Object> = data
        .iter()
        .map(|d| {
            let mut f = dictionary! { "T" => Object::string_literal(d.name.clone()) };
            match d.value.len() {
                0 => {}
                1 => f.set("V", Object::string_literal(d.value[0].clone())),
                _ => f.set(
                    "V",
                    Object::Array(
                        d.value.iter().map(|v| Object::string_literal(v.clone())).collect(),
                    ),
                ),
            }
            Object::Dictionary(f)
        })
        .collect();
    let fdf_id = doc.add_object(dictionary! { "Fields" => Object::Array(fields) });
    let catalog_id = doc.add_object(dictionary! { "FDF" => Object::Reference(fdf_id) });
    doc.trailer.set("Root", catalog_id);

    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| CommandError::PdfError(format!("fdf save: {e}")))?;
    // An FDF file is a PDF-syntax file with an FDF header.
    if out.starts_with(b"%PDF-") {
        let mut fixed = b"%FDF-1.2".to_vec();
        fixed.extend_from_slice(&out[b"%PDF-1.2".len()..]);
        return Ok(fixed);
    }
    Ok(out)
}

/// Escape the five XML entities.
fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// SPEC: P5-FORM-008 — serialise as **XFDF** (XML). Multi-value fields emit one
/// `<value>` element per value.
#[must_use]
pub fn to_xfdf(data: &[FormDatum]) -> String {
    let mut s = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    s.push_str("<xfdf xmlns=\"http://ns.adobe.com/xfdf/\" xml:space=\"preserve\">\n  <fields>\n");
    for d in data {
        let _ = writeln!(s, "    <field name=\"{}\">", xml_escape(&d.name));
        for v in &d.value {
            let _ = writeln!(s, "      <value>{}</value>", xml_escape(v));
        }
        s.push_str("    </field>\n");
    }
    s.push_str("  </fields>\n</xfdf>\n");
    s
}

/// SPEC: P5-FORM-008 — serialise as **JSON**: `[{name, type, value: […]}]`.
pub fn to_json(data: &[FormDatum]) -> Result<String, CommandError> {
    serde_json::to_string_pretty(data)
        .map_err(|e| CommandError::Internal(format!("json encode: {e}")))
}

/// RFC 4180: quote a field when it holds a comma, quote, CR or LF; double inner quotes.
fn csv_quote(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_owned()
    }
}

/// SPEC: P5-FORM-008 — serialise as **CSV** (`name,type,value`). Multi-select
/// values are joined with `;` inside the single value cell.
#[must_use]
pub fn to_csv(data: &[FormDatum]) -> String {
    let mut s = String::from("name,type,value\n");
    for d in data {
        let _ = writeln!(
            s,
            "{},{},{}",
            csv_quote(&d.name),
            csv_quote(&d.kind),
            csv_quote(&d.value.join(";"))
        );
    }
    s
}

/// SPEC: P5-FORM-008 — render `data` in `format`.
pub fn serialize(data: &[FormDatum], format: ExportFormat) -> Result<Vec<u8>, CommandError> {
    Ok(match format {
        ExportFormat::Fdf => to_fdf(data)?,
        ExportFormat::Xfdf => to_xfdf(data).into_bytes(),
        ExportFormat::Json => to_json(data)?.into_bytes(),
        ExportFormat::Csv => to_csv(data).into_bytes(),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::{csv_quote, to_csv, to_xfdf, xml_escape, FormDatum};

    fn datum(name: &str, kind: &str, value: &[&str]) -> FormDatum {
        FormDatum {
            name: name.to_owned(),
            kind: kind.to_owned(),
            value: value.iter().map(|v| (*v).to_owned()).collect(),
        }
    }

    #[test]
    fn escapes_xml_entities() {
        assert_eq!(xml_escape("a&b<c>\"d\'"), "a&amp;b&lt;c&gt;&quot;d&apos;");
    }

    #[test]
    fn xfdf_emits_one_value_per_entry() {
        let x = to_xfdf(&[datum("colors", "list", &["Red", "Blue"])]);
        assert_eq!(x.matches("<value>").count(), 2);
        assert!(x.contains("name=\"colors\""));
    }

    #[test]
    fn csv_quotes_commas_and_quotes() {
        assert_eq!(csv_quote("plain"), "plain");
        assert_eq!(csv_quote("a,b"), "\"a,b\"");
        assert_eq!(csv_quote("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn csv_joins_multi_values() {
        let c = to_csv(&[datum("colors", "list", &["Red", "Blue"])]);
        assert!(c.contains("colors,list,Red;Blue"), "{c}");
        assert!(c.starts_with("name,type,value\n"));
    }
}
