//! Interactive-form (`AcroForm`) detection — the Phase 5 foundation.
//!
//! SPEC: P5-FORM-001 — "WHEN the user opens a PDF containing `AcroForm` fields,
//! THE system SHALL detect them and display a 'Form mode' entry point with field
//! count." This is a read-only COS query (lopdf), the sibling of
//! [`crate::pdf::cos::read_text_boxes_doc`]: it resolves the catalog's
//! `/AcroForm`, counts the **terminal (fillable) fields**, and flags `/XFA`.
//!
//! "Field count" is deliberately the number of *terminal* fields — the leaves a
//! user actually fills. A radio group (one field with several widget kids) counts
//! once; a hierarchical container (a field whose kids are themselves fields) is
//! not counted, only its leaves are. Bare `/Widget` annotations are never fields.

use std::collections::HashSet;

use lopdf::{Dictionary, Document, Object, ObjectId, StringFormat};
use pdfium_render::prelude::PdfDocument;

use crate::error::CommandError;
use crate::pdf::cos::acroform_dict;
use crate::pdf::document::{pdfium, pdfium_lock};
use crate::pdf::restore::RestoreDocEdit;
use crate::pdf::undo::Edit;

/// `.map_err` adapter for lopdf errors (cos keeps its own private copy).
#[allow(clippy::needless_pass_by_value)]
fn lop(e: lopdf::Error) -> CommandError {
    CommandError::PdfError(format!("lopdf: {e}"))
}

/// A document's interactive-form summary, as the frontend's "Form mode" entry
/// point consumes it. Crosses the IPC boundary as camelCase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormSummary {
    /// Count of terminal (fillable) `AcroForm` fields. `0` ⇒ no fillable form.
    pub field_count: u32,
    /// Whether the form carries an `/XFA` entry. XFA fill/convert is P5.A5;
    /// A1 only reports it so the UI can later degrade gracefully.
    pub has_xfa: bool,
}

impl FormSummary {
    const NONE: Self = Self { field_count: 0, has_xfa: false };
}

/// A field-tree node is a *field* (as opposed to a bare widget annotation) when
/// it declares a field type (`/FT`) or a partial name (`/T`).
fn is_field(dict: &Dictionary) -> bool {
    dict.get(b"FT").is_ok() || dict.get(b"T").is_ok()
}

/// Resolve a `/Fields` or `/Kids` entry to its dictionary, whether it is stored
/// as an indirect reference or inline. `None` for anything that isn't a dict.
fn node_dict<'a>(doc: &'a Document, obj: &'a Object) -> Option<&'a Dictionary> {
    match obj {
        Object::Reference(id) => doc.get_dictionary(*id).ok(),
        Object::Dictionary(d) => Some(d),
        _ => None,
    }
}

/// SPEC: P5-FORM-001 — summarise the live document's interactive form. Read-only.
/// Skips malformed nodes rather than failing (the "skip one, don't fail the read"
/// convention); a cycle/size guard bounds the field-tree walk.
pub fn read_form_summary_doc(doc: &Document) -> Result<FormSummary, CommandError> {
    let Some(acro) = acroform_dict(doc)? else {
        return Ok(FormSummary::NONE);
    };
    let has_xfa = acro.get(b"XFA").is_ok();
    let Some(fields) = acro.get(b"Fields").ok().and_then(|o| o.as_array().ok()) else {
        return Ok(FormSummary { field_count: 0, has_xfa });
    };

    let mut count: u32 = 0;
    let mut visited: HashSet<ObjectId> = HashSet::new();
    // Work-list of field nodes to classify — iterative, so a deep or malformed
    // tree can't blow the stack. `budget` caps a pathological `/Kids` cycle.
    let mut stack: Vec<Object> = fields.clone();
    let mut budget: u32 = 100_000;

    while let Some(obj) = stack.pop() {
        budget = budget.saturating_sub(1);
        if budget == 0 {
            break;
        }
        if let Object::Reference(id) = obj {
            if !visited.insert(id) {
                continue; // already walked this field — a reference cycle
            }
        }
        let Some(dict) = node_dict(doc, &obj) else {
            continue;
        };
        // Kids that are themselves fields (not bare widgets) make this a
        // container; otherwise this node is a terminal (fillable) field.
        let field_kids: Vec<Object> = dict
            .get(b"Kids")
            .ok()
            .and_then(|o| o.as_array().ok())
            .map(|kids| {
                kids.iter()
                    .filter(|k| node_dict(doc, k).is_some_and(is_field))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        if field_kids.is_empty() {
            count = count.saturating_add(1);
        } else {
            stack.extend(field_kids);
        }
    }

    Ok(FormSummary { field_count: count, has_xfa })
}

/// Byte-level convenience wrapper around [`read_form_summary_doc`] — parses
/// `bytes` with lopdf, then summarises. Used by tests and any caller that has
/// bytes rather than a live document.
pub fn read_form_summary(bytes: &[u8]) -> Result<FormSummary, CommandError> {
    let doc = Document::load_mem(bytes).map_err(lop)?;
    read_form_summary_doc(&doc)
}

// ── P5.A2 — fill text fields ────────────────────────────────────────────────

/// One fillable text field on a page, as the fill overlay consumes it. Geometry
/// is in PDF points (page space, origin bottom-left) — the same space the text
/// overlays use, so a widget can be positioned via `pdfToScreen`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormField {
    /// Fully-qualified field name (parent partial names joined by `.`) — the
    /// stable handle the fill write addresses.
    pub name: String,
    /// Widget bounds `[x0, y0, x1, y1]`.
    pub rect: [f32; 4],
    /// Current value (`/V`), decoded from UTF-16BE-BOM or `PDFDocEncoding`.
    pub value: String,
    /// `/MaxLen` when the field declares one (character cap).
    pub max_len: Option<u32>,
    /// `/Ff` bit 13 (multi-line text).
    pub multiline: bool,
}

/// Field-flag bit 13 (1-indexed) = multi-line text (`Ff & (1 << 12)`).
const FF_MULTILINE: i64 = 1 << 12;

/// SPEC: P5-FORM-002 (P5.A2) — every fillable **text** field (`/FT /Tx`) whose
/// widget sits on `page` (0-based), with geometry + current value. Read-only.
/// A field's type/value/max-len/flags may be inherited from a `/Parent`, so each
/// is resolved up the parent chain. Non-text widgets and malformed nodes are
/// skipped (the "skip one, don't fail the read" convention).
pub fn read_text_fields_doc(doc: &Document, page: usize) -> Result<Vec<FormField>, CommandError> {
    if acroform_dict(doc)?.is_none() {
        return Ok(Vec::new());
    }
    let page_no = u32::try_from(page)
        .map(|n| n + 1)
        .map_err(|_| CommandError::InvalidInput(format!("bad page index: {page}")))?;
    let Some(&page_id) = doc.get_pages().get(&page_no) else {
        return Ok(Vec::new());
    };
    let annots = doc.get_dictionary(page_id).ok().and_then(|p| p.get(b"Annots").ok().cloned());
    let arr = match annots {
        Some(Object::Array(a)) => a,
        Some(Object::Reference(id)) => {
            doc.get_object(id).and_then(Object::as_array).cloned().unwrap_or_default()
        }
        _ => return Ok(Vec::new()),
    };

    let mut out = Vec::new();
    for obj in arr {
        let Ok(id) = obj.as_reference() else { continue };
        let Ok(dict) = doc.get_dictionary(id) else { continue };
        // Effective field type must be text.
        if inherited(doc, dict, b"FT").and_then(|o| o.as_name().ok()) != Some(b"Tx".as_slice()) {
            continue;
        }
        let Some(rect) = dict_rect(dict, b"Rect") else { continue };
        let value = inherited(doc, dict, b"V")
            .and_then(|o| o.as_str().ok())
            .map(decode_pdf_text_string)
            .unwrap_or_default();
        let max_len = inherited(doc, dict, b"MaxLen")
            .and_then(|o| o.as_i64().ok())
            .and_then(|n| u32::try_from(n).ok());
        let multiline = inherited(doc, dict, b"Ff").and_then(|o| o.as_i64().ok()).unwrap_or(0)
            & FF_MULTILINE
            != 0;
        out.push(FormField {
            name: qualified_name(doc, dict).unwrap_or_default(),
            rect,
            value,
            max_len,
            multiline,
        });
    }
    Ok(out)
}

/// Byte-level wrapper around [`read_text_fields_doc`].
pub fn read_text_fields(bytes: &[u8], page: usize) -> Result<Vec<FormField>, CommandError> {
    let doc = Document::load_mem(bytes).map_err(lop)?;
    read_text_fields_doc(&doc, page)
}

/// SPEC: P5-FORM-002 — set the text field named `name` to `value`, truncated to
/// the field's `/MaxLen`. Sets `/V`, drops the widget's stale `/AP`, and flips
/// `AcroForm` `/NeedAppearances` so viewers regenerate the appearance. Returns the
/// re-serialised bytes; verification is by re-reading `/V`.
pub fn set_text_field_value(bytes: &[u8], name: &str, value: &str) -> Result<Vec<u8>, CommandError> {
    let mut doc = Document::load_mem(bytes).map_err(lop)?;

    let target = find_field_by_name(&doc, name)
        .ok_or_else(|| CommandError::NotFound(format!("form field: {name}")))?;

    // Truncate to /MaxLen (character count) before writing.
    let max_len = {
        let dict = doc.get_dictionary(target).map_err(lop)?;
        inherited(&doc, dict, b"MaxLen").and_then(|o| o.as_i64().ok()).and_then(|n| usize::try_from(n).ok())
    };
    let value: String = match max_len {
        Some(max) => value.chars().take(max).collect(),
        None => value.to_owned(),
    };

    // Set /V on the field and drop its own stale appearance.
    {
        let dict = doc.get_dictionary_mut(target).map_err(lop)?;
        dict.set("V", encode_pdf_text_string(&value));
        dict.remove(b"AP");
    }
    // Drop /AP on any kid widgets (separate field/widget case).
    for kid in kid_widget_ids(&doc, target) {
        if let Ok(k) = doc.get_dictionary_mut(kid) {
            k.remove(b"AP");
        }
    }
    set_need_appearances(&mut doc)?;

    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| CommandError::PdfError(format!("lopdf save: {e}")))?;
    Ok(out)
}

/// Locate a field by its fully-qualified name, walking `/Fields` and field-`/Kids`
/// (an iterative worklist with a visited-set, like the count in `read_form_summary_doc`).
fn find_field_by_name(doc: &Document, name: &str) -> Option<ObjectId> {
    let acro = acroform_dict(doc).ok()??;
    let fields = acro.get(b"Fields").ok()?.as_array().ok()?;
    let mut stack: Vec<ObjectId> = fields.iter().filter_map(|o| o.as_reference().ok()).collect();
    let mut visited: HashSet<ObjectId> = HashSet::new();
    while let Some(id) = stack.pop() {
        if !visited.insert(id) {
            continue;
        }
        let Ok(dict) = doc.get_dictionary(id) else { continue };
        if qualified_name(doc, dict).as_deref() == Some(name) {
            return Some(id);
        }
        if let Ok(kids) = dict.get(b"Kids").and_then(Object::as_array) {
            for k in kids {
                if let Ok(kid_id) = k.as_reference() {
                    let is_field = doc
                        .get_dictionary(kid_id)
                        .is_ok_and(|d| d.get(b"FT").is_ok() || d.get(b"T").is_ok());
                    if is_field {
                        stack.push(kid_id);
                    }
                }
            }
        }
    }
    None
}

/// The reference ids of a field's `/Kids` (widget or child-field), for `/AP` clearing.
fn kid_widget_ids(doc: &Document, field: ObjectId) -> Vec<ObjectId> {
    doc.get_dictionary(field)
        .ok()
        .and_then(|d| d.get(b"Kids").ok())
        .and_then(|o| o.as_array().ok())
        .map(|kids| kids.iter().filter_map(|k| k.as_reference().ok()).collect())
        .unwrap_or_default()
}

/// Flip `AcroForm` `/NeedAppearances true` so viewers regenerate field appearances,
/// whether the form is stored as a reference or inline in the catalog.
fn set_need_appearances(doc: &mut Document) -> Result<(), CommandError> {
    let root = doc.trailer.get(b"Root").and_then(Object::as_reference).map_err(lop)?;
    let acro = doc.get_dictionary(root).map_err(lop)?.get(b"AcroForm").ok().cloned();
    match acro {
        Some(Object::Reference(id)) => {
            doc.get_dictionary_mut(id).map_err(lop)?.set("NeedAppearances", true);
        }
        Some(Object::Dictionary(_)) => {
            if let Ok(Object::Dictionary(a)) = doc.get_dictionary_mut(root).map_err(lop)?.get_mut(b"AcroForm") {
                a.set("NeedAppearances", true);
            }
        }
        _ => {}
    }
    Ok(())
}

/// Resolve an inheritable field attribute, walking the `/Parent` chain (capped).
fn inherited<'a>(doc: &'a Document, start: &'a Dictionary, key: &[u8]) -> Option<&'a Object> {
    let mut cur = start;
    for _ in 0..32 {
        if let Ok(v) = cur.get(key) {
            return Some(v);
        }
        let parent = cur.get(b"Parent").ok()?.as_reference().ok()?;
        cur = doc.get_dictionary(parent).ok()?;
    }
    None
}

/// A field's fully-qualified name: partial `/T`s from root to leaf joined by `.`.
fn qualified_name(doc: &Document, start: &Dictionary) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut cur = start;
    for _ in 0..32 {
        if let Ok(t) = cur.get(b"T").and_then(Object::as_str) {
            parts.push(decode_pdf_text_string(t));
        }
        let Ok(parent) = cur.get(b"Parent").and_then(Object::as_reference) else { break };
        let Ok(p) = doc.get_dictionary(parent) else { break };
        cur = p;
    }
    if parts.is_empty() {
        return None;
    }
    parts.reverse();
    Some(parts.join("."))
}

/// Decode a PDF text string: UTF-16BE when it carries the `FEFF` BOM, else
/// `PDFDocEncoding` approximated as Latin-1 (good enough for field values).
fn decode_pdf_text_string(raw: &[u8]) -> String {
    if raw.len() >= 2 && raw[0] == 0xFE && raw[1] == 0xFF {
        let units: Vec<u16> =
            raw[2..].chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&units)
    } else {
        raw.iter().map(|&b| b as char).collect()
    }
}

/// Encode a string as a PDF text string: a literal for ASCII, else UTF-16BE with
/// a `FEFF` BOM (what Acrobat expects for non-Latin field values).
fn encode_pdf_text_string(s: &str) -> Object {
    if s.is_ascii() {
        Object::string_literal(s)
    } else {
        let mut bytes = vec![0xFE, 0xFF];
        for unit in s.encode_utf16() {
            bytes.extend_from_slice(&unit.to_be_bytes());
        }
        Object::String(bytes, StringFormat::Literal)
    }
}

#[allow(clippy::cast_precision_loss)]
fn dict_rect(d: &Dictionary, key: &[u8]) -> Option<[f32; 4]> {
    let Ok(Object::Array(a)) = d.get(key) else { return None };
    if a.len() != 4 {
        return None;
    }
    let mut out = [0.0f32; 4];
    for (slot, obj) in out.iter_mut().zip(a) {
        *slot = match obj {
            Object::Real(v) => *v,
            Object::Integer(v) => *v as f32,
            _ => return None,
        };
    }
    Some(out)
}

/// SPEC: P5-FORM-002 — fill a text field as one undoable edit. The inverse is a
/// pre-write byte snapshot (`RestoreDocEdit`), the same chassis as `WatermarkEdit`.
pub struct FillTextFieldEdit {
    pub name: String,
    pub value: String,
}

impl<'a> Edit<PdfDocument<'a>> for FillTextFieldEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        let pre_bytes = {
            let _guard = pdfium_lock()?;
            doc.save_to_bytes().map_err(CommandError::from)?
        };
        let new_bytes = set_text_field_value(&pre_bytes, &self.name, &self.value)?;
        {
            let _guard = pdfium_lock()?;
            *doc = pdfium()?.load_pdf_from_byte_vec(new_bytes, None).map_err(CommandError::from)?;
        }
        Ok(Box::new(RestoreDocEdit { bytes: pre_bytes }))
    }

    fn label(&self) -> &'static str {
        "fill-text-field"
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::read_form_summary_doc;
    use lopdf::{dictionary, Document, Object};

    /// Attach a minimal catalog + `/AcroForm` (with `fields` and any `extra`
    /// `AcroForm` entries) to a doc that already holds the referenced field
    /// objects. Returns the finished doc.
    fn with_form(mut doc: Document, fields: Vec<Object>, extra: &[(&str, Object)]) -> Document {
        let mut acro = dictionary! { "Fields" => fields };
        for (k, v) in extra {
            acro.set(*k, v.clone());
        }
        let acro_id = doc.add_object(acro);
        let page_tree_id = doc.new_object_id();
        let page_id = doc.add_object(dictionary! { "Type" => "Page", "Parent" => page_tree_id });
        doc.objects.insert(
            page_tree_id,
            Object::Dictionary(
                dictionary! { "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1 },
            ),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog", "Pages" => page_tree_id, "AcroForm" => acro_id,
        });
        doc.trailer.set("Root", catalog_id);
        doc
    }

    #[test]
    fn no_acroform_reports_zero() {
        let mut doc = Document::with_version("1.5");
        let page_tree_id = doc.new_object_id();
        let page_id = doc.add_object(dictionary! { "Type" => "Page", "Parent" => page_tree_id });
        doc.objects.insert(
            page_tree_id,
            Object::Dictionary(
                dictionary! { "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1 },
            ),
        );
        let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => page_tree_id });
        doc.trailer.set("Root", catalog_id);

        let s = read_form_summary_doc(&doc).unwrap();
        assert_eq!(s.field_count, 0);
        assert!(!s.has_xfa);
    }

    #[test]
    fn two_flat_fields_count_two() {
        let mut doc = Document::with_version("1.5");
        let f1 = doc.add_object(dictionary! { "FT" => "Tx", "T" => Object::string_literal("first") });
        let f2 = doc.add_object(dictionary! { "FT" => "Tx", "T" => Object::string_literal("last") });
        let doc = with_form(doc, vec![f1.into(), f2.into()], &[]);
        assert_eq!(read_form_summary_doc(&doc).unwrap().field_count, 2);
    }

    #[test]
    fn radio_group_counts_as_one() {
        // One `/Btn` field with three widget kids (no /FT, no /T) → one terminal.
        let mut doc = Document::with_version("1.5");
        let w1 = doc.add_object(dictionary! { "Subtype" => "Widget" });
        let w2 = doc.add_object(dictionary! { "Subtype" => "Widget" });
        let w3 = doc.add_object(dictionary! { "Subtype" => "Widget" });
        let group = doc.add_object(dictionary! {
            "FT" => "Btn", "T" => Object::string_literal("choice"),
            "Kids" => vec![w1.into(), w2.into(), w3.into()],
        });
        let doc = with_form(doc, vec![group.into()], &[]);
        assert_eq!(read_form_summary_doc(&doc).unwrap().field_count, 1);
    }

    #[test]
    fn hierarchical_fields_count_terminals() {
        // A container field whose two kids are themselves fields → count the kids.
        let mut doc = Document::with_version("1.5");
        let child_a = doc.add_object(dictionary! { "FT" => "Tx", "T" => Object::string_literal("a") });
        let child_b = doc.add_object(dictionary! { "FT" => "Tx", "T" => Object::string_literal("b") });
        let parent = doc.add_object(dictionary! {
            "T" => Object::string_literal("group"),
            "Kids" => vec![child_a.into(), child_b.into()],
        });
        let doc = with_form(doc, vec![parent.into()], &[]);
        assert_eq!(read_form_summary_doc(&doc).unwrap().field_count, 2);
    }

    #[test]
    fn xfa_flag_detected() {
        let doc = with_form(Document::with_version("1.5"), vec![], &[(
            "XFA",
            Object::string_literal("<xdp/>"),
        )]);
        let s = read_form_summary_doc(&doc).unwrap();
        assert_eq!(s.field_count, 0);
        assert!(s.has_xfa);
    }
}
