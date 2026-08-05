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

use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, StringFormat};
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

// ── P5.A3 — fill checkbox / radio ───────────────────────────────────────────

/// `/Ff` bit 16 (1-indexed) = radio; bit 17 = pushbutton (no value).
const FF_RADIO: i64 = 1 << 15;
const FF_PUSHBUTTON: i64 = 1 << 16;

/// One clickable button widget (a checkbox, or one option of a radio group), as
/// the button overlay consumes it. Geometry is in PDF points.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ButtonField {
    /// Fully-qualified name of the owning field (the *group* for a radio option).
    pub field_name: String,
    /// `"checkbox"` or `"radio"`.
    pub kind: String,
    /// Widget bounds `[x0, y0, x1, y1]`.
    pub rect: [f32; 4],
    /// This widget's "on" state — the non-`/Off` key of its `/AP /N`.
    pub on_state: String,
    /// Whether this widget is currently on (the field's `/V` equals `on_state`).
    pub checked: bool,
}

/// This widget's "on" appearance-state name: the non-`/Off` key of `/AP /N`.
fn widget_on_state(doc: &Document, dict: &Dictionary) -> Option<String> {
    let ap = dict.get(b"AP").ok().and_then(|o| node_dict(doc, o))?;
    let n = ap.get(b"N").ok().and_then(|o| node_dict(doc, o))?;
    n.iter()
        .map(|(k, _)| k.as_slice())
        .find(|k| *k != b"Off")
        .map(|k| String::from_utf8_lossy(k).into_owned())
}

/// SPEC: P5-FORM-003 (P5.A3) — every fillable button widget (checkbox or radio
/// option, `/FT /Btn`, not a pushbutton) on `page` (0-based), with geometry, its
/// on-state, and whether it's currently on. Read-only. Widgets that declare no
/// `/AP /N` appearance states are skipped (nothing to toggle).
pub fn read_button_fields_doc(doc: &Document, page: usize) -> Result<Vec<ButtonField>, CommandError> {
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
        if inherited(doc, dict, b"FT").and_then(|o| o.as_name().ok()) != Some(b"Btn".as_slice()) {
            continue;
        }
        let ff = inherited(doc, dict, b"Ff").and_then(|o| o.as_i64().ok()).unwrap_or(0);
        if ff & FF_PUSHBUTTON != 0 {
            continue; // pushbuttons carry no value
        }
        let Some(on_state) = widget_on_state(doc, dict) else { continue };
        let Some(rect) = dict_rect(dict, b"Rect") else { continue };
        let value = inherited(doc, dict, b"V")
            .and_then(|o| o.as_name().ok())
            .map(|n| String::from_utf8_lossy(n).into_owned());
        let checked = value.as_deref() == Some(on_state.as_str());
        out.push(ButtonField {
            field_name: qualified_name(doc, dict).unwrap_or_default(),
            kind: (if ff & FF_RADIO != 0 { "radio" } else { "checkbox" }).to_owned(),
            rect,
            on_state,
            checked,
        });
    }
    Ok(out)
}

/// Byte-level wrapper around [`read_button_fields_doc`].
pub fn read_button_fields(bytes: &[u8], page: usize) -> Result<Vec<ButtonField>, CommandError> {
    let doc = Document::load_mem(bytes).map_err(lop)?;
    read_button_fields_doc(&doc, page)
}

/// The widget object ids that carry a button field's appearance: its `/Kids`
/// (radio options / separate widgets), or the field itself when it is its own
/// widget (a merged checkbox with no kids).
fn field_widget_ids(doc: &Document, field: ObjectId) -> Vec<ObjectId> {
    let kids = kid_widget_ids(doc, field);
    if kids.is_empty() {
        vec![field]
    } else {
        kids
    }
}

/// SPEC: P5-FORM-003 — set button field `name` on/off to `on_state`. Sets the
/// field's `/V` and each widget's `/AS` (the pre-baked `/AP /N` appearance is
/// selected by `/AS`, so — unlike text — `/NeedAppearances` is *not* touched).
/// `checked` false turns it off (`/Off`), which also deselects a radio group.
pub fn set_button_field(
    bytes: &[u8],
    name: &str,
    on_state: &str,
    checked: bool,
) -> Result<Vec<u8>, CommandError> {
    let mut doc = Document::load_mem(bytes).map_err(lop)?;
    let field = find_field_by_name(&doc, name)
        .ok_or_else(|| CommandError::NotFound(format!("form field: {name}")))?;

    // Compute each widget's target /AS first (immutable), then apply (mutable).
    let widgets = field_widget_ids(&doc, field);
    let targets: Vec<(ObjectId, Vec<u8>)> = widgets
        .iter()
        .map(|&wid| {
            let wid_on = doc.get_dictionary(wid).ok().and_then(|d| widget_on_state(&doc, d));
            let as_name = if checked && wid_on.as_deref() == Some(on_state) {
                on_state.as_bytes().to_vec()
            } else {
                b"Off".to_vec()
            };
            (wid, as_name)
        })
        .collect();

    let field_value = if checked { on_state.as_bytes().to_vec() } else { b"Off".to_vec() };
    doc.get_dictionary_mut(field).map_err(lop)?.set("V", Object::Name(field_value));
    for (wid, as_name) in targets {
        if let Ok(w) = doc.get_dictionary_mut(wid) {
            w.set("AS", Object::Name(as_name));
        }
    }

    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| CommandError::PdfError(format!("lopdf save: {e}")))?;
    Ok(out)
}

/// SPEC: P5-FORM-003 — toggle/select a button field as one undoable edit. Same
/// snapshot-inverse chassis as [`FillTextFieldEdit`].
pub struct SetButtonFieldEdit {
    pub name: String,
    pub on_state: String,
    pub checked: bool,
}

impl<'a> Edit<PdfDocument<'a>> for SetButtonFieldEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        let pre_bytes = {
            let _guard = pdfium_lock()?;
            doc.save_to_bytes().map_err(CommandError::from)?
        };
        let new_bytes = set_button_field(&pre_bytes, &self.name, &self.on_state, self.checked)?;
        {
            let _guard = pdfium_lock()?;
            *doc = pdfium()?.load_pdf_from_byte_vec(new_bytes, None).map_err(CommandError::from)?;
        }
        Ok(Box::new(RestoreDocEdit { bytes: pre_bytes }))
    }

    fn label(&self) -> &'static str {
        "set-button-field"
    }
}

// ── P5.A4 — fill choice fields (combo / list) ───────────────────────────────

/// `/Ff` bit 18 = combo (dropdown); bit 22 = multi-select (list only).
const FF_COMBO: i64 = 1 << 17;
const FF_MULTISELECT: i64 = 1 << 21;

/// One option of a choice field: its export value (stored in `/V`) and the label
/// shown to the user. Equal when the `/Opt` entry is a bare string.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChoiceOption {
    pub export: String,
    pub label: String,
}

/// A combo box or list box, as the choice overlay consumes it.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChoiceField {
    /// Fully-qualified field name.
    pub name: String,
    /// `"combo"` (dropdown) or `"list"`.
    pub kind: String,
    /// Widget bounds `[x0, y0, x1, y1]`.
    pub rect: [f32; 4],
    /// The declared options, in `/Opt` order.
    pub options: Vec<ChoiceOption>,
    /// Currently-selected export values (from `/V`).
    pub selected: Vec<String>,
    /// Multi-select (list boxes only).
    pub multi: bool,
}

/// Follow an indirect reference to its object; other objects pass through.
fn deref<'a>(doc: &'a Document, obj: &'a Object) -> &'a Object {
    obj.as_reference().ok().and_then(|id| doc.get_object(id).ok()).unwrap_or(obj)
}

/// Parse a choice field's `/Opt` into options. Each entry is a text string
/// (export == label) or a two-element array `[export display]` (PDF 32000
/// Table 231).
fn parse_opt(doc: &Document, opt: Option<&Object>) -> Vec<ChoiceOption> {
    let Some(Object::Array(arr)) = opt.map(|o| deref(doc, o)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in arr {
        match deref(doc, entry) {
            Object::String(s, _) => {
                let text = decode_pdf_text_string(s);
                out.push(ChoiceOption { export: text.clone(), label: text });
            }
            Object::Array(pair) if pair.len() == 2 => {
                let export = pair[0].as_str().map(decode_pdf_text_string).unwrap_or_default();
                let label = pair[1].as_str().map(decode_pdf_text_string).unwrap_or_default();
                out.push(ChoiceOption { export, label });
            }
            _ => {}
        }
    }
    out
}

/// Parse a choice field's `/V` into the selected export values (a single string,
/// an array of strings, or occasionally a Name).
fn parse_choice_value(doc: &Document, v: Option<&Object>) -> Vec<String> {
    match v.map(|o| deref(doc, o)) {
        Some(Object::String(s, _)) => vec![decode_pdf_text_string(s)],
        Some(Object::Name(n)) => vec![decode_pdf_text_string(n)],
        Some(Object::Array(a)) => {
            a.iter().filter_map(|e| e.as_str().ok().map(decode_pdf_text_string)).collect()
        }
        _ => Vec::new(),
    }
}

/// SPEC: P5-FORM-004 (P5.A4) — every choice field (`/FT /Ch`) whose widget sits
/// on `page` (0-based), with its options + current selection. Read-only.
pub fn read_choice_fields_doc(doc: &Document, page: usize) -> Result<Vec<ChoiceField>, CommandError> {
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
        if inherited(doc, dict, b"FT").and_then(|o| o.as_name().ok()) != Some(b"Ch".as_slice()) {
            continue;
        }
        let Some(rect) = dict_rect(dict, b"Rect") else { continue };
        let ff = inherited(doc, dict, b"Ff").and_then(|o| o.as_i64().ok()).unwrap_or(0);
        out.push(ChoiceField {
            name: qualified_name(doc, dict).unwrap_or_default(),
            kind: (if ff & FF_COMBO != 0 { "combo" } else { "list" }).to_owned(),
            rect,
            options: parse_opt(doc, inherited(doc, dict, b"Opt")),
            selected: parse_choice_value(doc, inherited(doc, dict, b"V")),
            multi: ff & FF_MULTISELECT != 0,
        });
    }
    Ok(out)
}

/// Byte-level wrapper around [`read_choice_fields_doc`].
pub fn read_choice_fields(bytes: &[u8], page: usize) -> Result<Vec<ChoiceField>, CommandError> {
    let doc = Document::load_mem(bytes).map_err(lop)?;
    read_choice_fields_doc(&doc, page)
}

/// SPEC: P5-FORM-004 — set choice field `name`'s selection to `values` (export
/// values, each of which must be a declared `/Opt` option). Sets `/V` (a string
/// for one, an array for many), `/I` (selected indices, list boxes), and
/// `/NeedAppearances`. Returns the re-serialised bytes.
pub fn set_choice_field(bytes: &[u8], name: &str, values: &[String]) -> Result<Vec<u8>, CommandError> {
    let mut doc = Document::load_mem(bytes).map_err(lop)?;
    let field = find_field_by_name(&doc, name)
        .ok_or_else(|| CommandError::NotFound(format!("form field: {name}")))?;

    let (options, is_list) = {
        let dict = doc.get_dictionary(field).map_err(lop)?;
        let opts = parse_opt(&doc, inherited(&doc, dict, b"Opt"));
        let ff = inherited(&doc, dict, b"Ff").and_then(|o| o.as_i64().ok()).unwrap_or(0);
        (opts, ff & FF_COMBO == 0)
    };
    // Reject any value not declared in /Opt (an editable combo would relax this;
    // A4 offers only the declared options).
    if let Some(bad) = values.iter().find(|v| !options.iter().any(|o| o.export == **v)) {
        return Err(CommandError::InvalidInput(format!("not an option of {name}: {bad}")));
    }

    let v_obj = match values {
        [] => Object::string_literal(""),
        [one] => encode_pdf_text_string(one),
        many => Object::Array(many.iter().map(|v| encode_pdf_text_string(v)).collect()),
    };
    doc.get_dictionary_mut(field).map_err(lop)?.set("V", v_obj);

    if is_list {
        let mut idx: Vec<i64> = values
            .iter()
            .filter_map(|v| options.iter().position(|o| o.export == *v))
            .filter_map(|p| i64::try_from(p).ok())
            .collect();
        idx.sort_unstable();
        doc.get_dictionary_mut(field)
            .map_err(lop)?
            .set("I", Object::Array(idx.into_iter().map(Object::Integer).collect()));
    }
    set_need_appearances(&mut doc)?;

    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| CommandError::PdfError(format!("lopdf save: {e}")))?;
    Ok(out)
}

/// SPEC: P5-FORM-004 — set a choice field's selection as one undoable edit. Same
/// snapshot-inverse chassis as [`FillTextFieldEdit`].
pub struct SetChoiceFieldEdit {
    pub name: String,
    pub values: Vec<String>,
}

impl<'a> Edit<PdfDocument<'a>> for SetChoiceFieldEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        let pre_bytes = {
            let _guard = pdfium_lock()?;
            doc.save_to_bytes().map_err(CommandError::from)?
        };
        let new_bytes = set_choice_field(&pre_bytes, &self.name, &self.values)?;
        {
            let _guard = pdfium_lock()?;
            *doc = pdfium()?.load_pdf_from_byte_vec(new_bytes, None).map_err(CommandError::from)?;
        }
        Ok(Box::new(RestoreDocEdit { bytes: pre_bytes }))
    }

    fn label(&self) -> &'static str {
        "set-choice-field"
    }
}

// ── P5.A5 — XFA degraded support ────────────────────────────────────────────

/// SPEC: P5-FORM-005 (P5.A5) — drop the dynamic XFA layer: remove `/XFA` from the
/// `AcroForm` and set `/NeedAppearances` so the document's static content (and any
/// static `AcroForm` fields) render. We don't render XFA; this is the honest
/// "convert to flat content (read-only)". Errors if there is no `/XFA` to strip.
pub fn strip_xfa(bytes: &[u8]) -> Result<Vec<u8>, CommandError> {
    let mut doc = Document::load_mem(bytes).map_err(lop)?;
    let root = doc.trailer.get(b"Root").and_then(Object::as_reference).map_err(lop)?;
    let acro = doc.get_dictionary(root).map_err(lop)?.get(b"AcroForm").ok().cloned();

    let removed = match acro {
        Some(Object::Reference(id)) => doc.get_dictionary_mut(id).map_err(lop)?.remove(b"XFA").is_some(),
        Some(Object::Dictionary(_)) => {
            match doc.get_dictionary_mut(root).map_err(lop)?.get_mut(b"AcroForm") {
                Ok(Object::Dictionary(a)) => a.remove(b"XFA").is_some(),
                _ => false,
            }
        }
        _ => false,
    };
    if !removed {
        return Err(CommandError::InvalidInput("document has no XFA form".into()));
    }
    set_need_appearances(&mut doc)?;

    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| CommandError::PdfError(format!("lopdf save: {e}")))?;
    Ok(out)
}

/// SPEC: P5-FORM-005 — strip the XFA layer as one undoable edit. Same
/// snapshot-inverse chassis as [`FillTextFieldEdit`].
pub struct StripXfaEdit;

impl<'a> Edit<PdfDocument<'a>> for StripXfaEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        let pre_bytes = {
            let _guard = pdfium_lock()?;
            doc.save_to_bytes().map_err(CommandError::from)?
        };
        let new_bytes = strip_xfa(&pre_bytes)?;
        {
            let _guard = pdfium_lock()?;
            *doc = pdfium()?.load_pdf_from_byte_vec(new_bytes, None).map_err(CommandError::from)?;
        }
        Ok(Box::new(RestoreDocEdit { bytes: pre_bytes }))
    }

    fn label(&self) -> &'static str {
        "strip-xfa"
    }
}

// ── P5.B1 — create text field ───────────────────────────────────────────────

/// `/Ff` bit 2 = required.
const FF_REQUIRED: i64 = 1 << 1;

/// Ensure the catalog has an indirect `AcroForm` with `/Fields`, a default `/DA`,
/// and a `/DR` font named `/Helv` (added without clobbering existing DR fonts).
/// Returns the `AcroForm` object id. Normalises an inline `AcroForm` to indirect.
fn ensure_acroform(doc: &mut Document) -> Result<ObjectId, CommandError> {
    let root = doc.trailer.get(b"Root").and_then(Object::as_reference).map_err(lop)?;
    let acro_id = match doc.get_dictionary(root).map_err(lop)?.get(b"AcroForm") {
        Ok(Object::Reference(id)) => *id,
        Ok(Object::Dictionary(d)) => {
            let d = d.clone();
            let id = doc.add_object(d);
            doc.get_dictionary_mut(root).map_err(lop)?.set("AcroForm", Object::Reference(id));
            id
        }
        _ => {
            let id = doc.add_object(dictionary! { "Fields" => Object::Array(Vec::new()) });
            doc.get_dictionary_mut(root).map_err(lop)?.set("AcroForm", Object::Reference(id));
            id
        }
    };

    let has_helv = doc
        .get_dictionary(acro_id)
        .ok()
        .and_then(|a| a.get(b"DR").ok())
        .and_then(|o| o.as_dict().ok())
        .and_then(|dr| dr.get(b"Font").ok())
        .and_then(|o| o.as_dict().ok())
        .is_some_and(|f| f.get(b"Helv").is_ok());
    if !has_helv {
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
        });
        let mut dr: Dictionary = doc
            .get_dictionary(acro_id)
            .ok()
            .and_then(|a| a.get(b"DR").ok())
            .and_then(|o| o.as_dict().ok())
            .cloned()
            .unwrap_or_default();
        let mut fonts: Dictionary =
            dr.get(b"Font").ok().and_then(|o| o.as_dict().ok()).cloned().unwrap_or_default();
        fonts.set("Helv", Object::Reference(font_id));
        dr.set("Font", Object::Dictionary(fonts));
        doc.get_dictionary_mut(acro_id).map_err(lop)?.set("DR", Object::Dictionary(dr));
    }
    let acro = doc.get_dictionary_mut(acro_id).map_err(lop)?;
    if acro.get(b"Fields").is_err() {
        acro.set("Fields", Object::Array(Vec::new()));
    }
    if acro.get(b"DA").is_err() {
        acro.set("DA", Object::string_literal("/Helv 0 Tf 0 g"));
    }
    Ok(acro_id)
}

/// Append `item` to the array at `dict[key]`, handling the array being inline or
/// an indirect reference, and creating it if absent.
fn append_ref(doc: &mut Document, dict_id: ObjectId, key: &[u8], item: ObjectId) -> Result<(), CommandError> {
    let arr_ref = doc
        .get_dictionary(dict_id)
        .ok()
        .and_then(|d| d.get(key).ok())
        .and_then(|o| o.as_reference().ok());
    if let Some(arr_id) = arr_ref {
        if let Ok(Object::Array(a)) = doc.get_object_mut(arr_id) {
            a.push(Object::Reference(item));
        }
        return Ok(());
    }
    let d = doc.get_dictionary_mut(dict_id).map_err(lop)?;
    match d.get_mut(key) {
        Ok(Object::Array(a)) => a.push(Object::Reference(item)),
        _ => d.set(key.to_vec(), Object::Array(vec![Object::Reference(item)])),
    }
    Ok(())
}

/// SPEC: P5-FORM-006 (P5.B1) — create a text field on `page` (0-based) at `rect`,
/// wired into the `AcroForm` and the page's `/Annots`. Rejects a duplicate
/// top-level field name. Sets `/NeedAppearances` so viewers render it.
#[allow(clippy::too_many_arguments)]
pub fn add_text_field(
    bytes: &[u8],
    page: usize,
    rect: [f32; 4],
    name: &str,
    default: &str,
    max_len: Option<u32>,
    multiline: bool,
    required: bool,
) -> Result<Vec<u8>, CommandError> {
    if name.trim().is_empty() {
        return Err(CommandError::InvalidInput("field name is required".into()));
    }
    let mut doc = Document::load_mem(bytes).map_err(lop)?;

    let page_no = u32::try_from(page)
        .map(|n| n + 1)
        .map_err(|_| CommandError::InvalidInput(format!("bad page index: {page}")))?;
    let page_id = *doc
        .get_pages()
        .get(&page_no)
        .ok_or_else(|| CommandError::InvalidInput(format!("no page {page}")))?;

    let acro_id = ensure_acroform(&mut doc)?;

    // Reject a colliding top-level field name (duplicate /T merges into one field).
    let duplicate = doc
        .get_dictionary(acro_id)
        .ok()
        .and_then(|a| a.get(b"Fields").ok())
        .and_then(|o| o.as_array().ok())
        .is_some_and(|fields| {
            fields
                .iter()
                .filter_map(|f| f.as_reference().ok())
                .filter_map(|id| doc.get_dictionary(id).ok())
                .filter_map(|d| d.get(b"T").and_then(Object::as_str).ok())
                .any(|t| String::from_utf8_lossy(t) == name)
        });
    if duplicate {
        return Err(CommandError::InvalidInput(format!("a form field named {name} already exists")));
    }

    let mut field = Dictionary::new();
    field.set("Type", "Annot");
    field.set("Subtype", "Widget");
    field.set("FT", "Tx");
    field.set("T", encode_pdf_text_string(name));
    field.set(
        "Rect",
        Object::Array(vec![
            Object::Real(rect[0]),
            Object::Real(rect[1]),
            Object::Real(rect[2]),
            Object::Real(rect[3]),
        ]),
    );
    field.set("P", Object::Reference(page_id));
    field.set("F", Object::Integer(4)); // Print
    field.set("DA", Object::string_literal("/Helv 0 Tf 0 g"));
    let mut ff = 0i64;
    if multiline {
        ff |= FF_MULTILINE;
    }
    if required {
        ff |= FF_REQUIRED;
    }
    if ff != 0 {
        field.set("Ff", Object::Integer(ff));
    }
    if !default.is_empty() {
        field.set("V", encode_pdf_text_string(default));
    }
    if let Some(ml) = max_len {
        field.set("MaxLen", Object::Integer(i64::from(ml)));
    }
    let field_id = doc.add_object(field);

    append_ref(&mut doc, page_id, b"Annots", field_id)?;
    append_ref(&mut doc, acro_id, b"Fields", field_id)?;
    set_need_appearances(&mut doc)?;

    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| CommandError::PdfError(format!("lopdf save: {e}")))?;
    Ok(out)
}

/// SPEC: P5-FORM-006 — create a text field as one undoable edit.
pub struct AddTextFieldEdit {
    pub page: usize,
    pub rect: [f32; 4],
    pub name: String,
    pub default: String,
    pub max_len: Option<u32>,
    pub multiline: bool,
    pub required: bool,
}

impl<'a> Edit<PdfDocument<'a>> for AddTextFieldEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        let pre_bytes = {
            let _guard = pdfium_lock()?;
            doc.save_to_bytes().map_err(CommandError::from)?
        };
        let new_bytes = add_text_field(
            &pre_bytes,
            self.page,
            self.rect,
            &self.name,
            &self.default,
            self.max_len,
            self.multiline,
            self.required,
        )?;
        {
            let _guard = pdfium_lock()?;
            *doc = pdfium()?.load_pdf_from_byte_vec(new_bytes, None).map_err(CommandError::from)?;
        }
        Ok(Box::new(RestoreDocEdit { bytes: pre_bytes }))
    }

    fn label(&self) -> &'static str {
        "add-text-field"
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
