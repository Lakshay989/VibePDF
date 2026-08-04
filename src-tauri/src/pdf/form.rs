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

use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::error::CommandError;
use crate::pdf::cos::acroform_dict;

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
    let doc = Document::load_mem(bytes)
        .map_err(|e| CommandError::PdfError(format!("lopdf: {e}")))?;
    read_form_summary_doc(&doc)
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
