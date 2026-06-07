//! COS (object-model) layer via `lopdf` — the structural edits `PDFium`'s
//! high-level API can't express.
//!
//! `PDFium` (`pdfium-render`) renders and does page-level operations, but its
//! API is read-only for the document `/Outlines` (bookmarks) and the
//! `/AcroForm` (interactive form fields), and it can't rewrite the page tree
//! or indirect references. `lopdf` is a pure-Rust read/write model of the PDF
//! object graph, so it can.
//!
//! **Integration model — byte handoff, never a shared handle.** Every function
//! here takes serialized PDF bytes and returns serialized PDF bytes. `PDFium` and
//! `lopdf` never hold the same live document at once; a structural edit is a
//! pass over a byte buffer that sits *between* `PDFium` passes. `lopdf` is pure
//! Rust, so this needs no `PDFIUM_LOCK`. Callers MUST round-trip the output
//! through `PDFium` (`verify_pdf_reopens`) before persisting it — the spike tests
//! assert that every output below reopens cleanly in `PDFium`.
//!
//! This module is the **capability spike** for the `lopdf` adoption (see
//! `docs/03_TECH_STACK.md` "Structural edits" and `docs/04_ARCHITECTURE.md`
//! "Structural edits via lopdf"). It proves the three operations the deferred
//! work needs — outline read, outline write, form-field rename — and is not yet
//! wired into any feature. It unblocks P2-PAGE-002 (reorder), P2-PAGE-003
//! (delete ref cleanup), P2-PAGE-005 (insert form fields), and P2-PAGE-008
//! (merge bookmarks + form-field rename).

use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::error::CommandError;

/// Map an `lopdf` error onto our typed error. Takes the error by value so it
/// can be used directly as a `.map_err(cos_err)` adapter.
#[allow(clippy::needless_pass_by_value)]
fn cos_err(e: lopdf::Error) -> CommandError {
    CommandError::PdfError(format!("lopdf: {e}"))
}

/// Read the titles of the top-level bookmarks (direct children of the document
/// `/Outlines`), in order. Empty when the document has no outline.
pub fn read_top_level_outline_titles(bytes: &[u8]) -> Result<Vec<String>, CommandError> {
    let doc = Document::load_mem(bytes).map_err(cos_err)?;

    let Some(outlines_id) = doc
        .catalog()
        .map_err(cos_err)?
        .get(b"Outlines")
        .ok()
        .and_then(|o| o.as_reference().ok())
    else {
        return Ok(Vec::new());
    };

    let mut titles = Vec::new();
    let mut cur = doc
        .get_dictionary(outlines_id)
        .map_err(cos_err)?
        .get(b"First")
        .ok()
        .and_then(|o| o.as_reference().ok());
    while let Some(id) = cur {
        let item = doc.get_dictionary(id).map_err(cos_err)?;
        if let Ok(t) = item.get(b"Title").and_then(Object::as_str) {
            titles.push(String::from_utf8_lossy(t).into_owned());
        }
        cur = item.get(b"Next").ok().and_then(|o| o.as_reference().ok());
    }
    Ok(titles)
}

/// Append a top-level bookmark titled `title` pointing at `page_index` (0-based)
/// to the document `/Outlines`, creating the outline if it doesn't exist.
/// Returns the re-serialized bytes. Proves the outline-write capability that
/// C4 (merge bookmarks) needs.
pub fn add_top_level_bookmark(
    bytes: &[u8],
    title: &str,
    page_index: u32,
) -> Result<Vec<u8>, CommandError> {
    let mut doc = Document::load_mem(bytes).map_err(cos_err)?;

    // `get_pages` is keyed by 1-based page number → page object id.
    let pages = doc.get_pages();
    let page_id = *pages.get(&(page_index + 1)).ok_or_else(|| {
        CommandError::InvalidInput(format!("page index out of range: {page_index}"))
    })?;

    // Ensure an /Outlines dictionary exists.
    let existing = doc
        .catalog()
        .map_err(cos_err)?
        .get(b"Outlines")
        .ok()
        .and_then(|o| o.as_reference().ok());
    let outlines_id = if let Some(id) = existing {
        id
    } else {
        let mut outlines = Dictionary::new();
        outlines.set("Type", Object::Name(b"Outlines".to_vec()));
        outlines.set("Count", Object::Integer(0));
        let id = doc.add_object(outlines);
        doc.catalog_mut()
            .map_err(cos_err)?
            .set("Outlines", Object::Reference(id));
        id
    };

    let last = doc
        .get_dictionary(outlines_id)
        .map_err(cos_err)?
        .get(b"Last")
        .ok()
        .and_then(|o| o.as_reference().ok());

    // Build the new item (a /Dest pointing at the page object, /Fit zoom).
    let mut item = Dictionary::new();
    item.set("Title", Object::string_literal(title));
    item.set("Parent", Object::Reference(outlines_id));
    item.set(
        "Dest",
        Object::Array(vec![Object::Reference(page_id), Object::Name(b"Fit".to_vec())]),
    );
    if let Some(last_id) = last {
        item.set("Prev", Object::Reference(last_id));
    }
    let item_id = doc.add_object(item);

    // Splice it onto the end of the sibling chain.
    if let Some(last_id) = last {
        doc.get_dictionary_mut(last_id)
            .map_err(cos_err)?
            .set("Next", Object::Reference(item_id));
        doc.get_dictionary_mut(outlines_id)
            .map_err(cos_err)?
            .set("Last", Object::Reference(item_id));
    } else {
        let outlines = doc.get_dictionary_mut(outlines_id).map_err(cos_err)?;
        outlines.set("First", Object::Reference(item_id));
        outlines.set("Last", Object::Reference(item_id));
    }

    // Bump /Count.
    {
        let outlines = doc.get_dictionary_mut(outlines_id).map_err(cos_err)?;
        let count = outlines.get(b"Count").ok().and_then(|c| c.as_i64().ok()).unwrap_or(0);
        outlines.set("Count", Object::Integer(count + 1));
    }

    let mut buf = Vec::new();
    doc.save_to(&mut buf)?;
    Ok(buf)
}

/// The object ids of the `/AcroForm` `/Fields` entries, handling `/AcroForm`
/// as either an indirect reference or an inline dictionary. Empty when there
/// is no form.
fn acroform_field_ids(doc: &Document) -> Result<Vec<ObjectId>, CommandError> {
    let catalog = doc.catalog().map_err(cos_err)?;
    let Ok(acroform) = catalog.get(b"AcroForm") else {
        return Ok(Vec::new());
    };
    let acro_dict = match acroform.as_reference() {
        Ok(id) => doc.get_dictionary(id).map_err(cos_err)?,
        Err(_) => acroform.as_dict().map_err(cos_err)?,
    };
    let ids = acro_dict
        .get(b"Fields")
        .ok()
        .and_then(|o| o.as_array().ok())
        .map(|arr| arr.iter().filter_map(|o| o.as_reference().ok()).collect())
        .unwrap_or_default();
    Ok(ids)
}

/// Read the names (`/T`) of the top-level form fields, in order.
pub fn read_form_field_names(bytes: &[u8]) -> Result<Vec<String>, CommandError> {
    let doc = Document::load_mem(bytes).map_err(cos_err)?;
    let mut names = Vec::new();
    for id in acroform_field_ids(&doc)? {
        if let Ok(t) = doc.get_dictionary(id).map_err(cos_err)?.get(b"T").and_then(Object::as_str) {
            names.push(String::from_utf8_lossy(t).into_owned());
        }
    }
    Ok(names)
}

/// Append `suffix` to every top-level form field name (`/T`). This is the
/// collision-resolution primitive merge needs (`name` → `name_2`). Returns the
/// re-serialized bytes.
pub fn rename_form_fields_with_suffix(
    bytes: &[u8],
    suffix: &str,
) -> Result<Vec<u8>, CommandError> {
    let mut doc = Document::load_mem(bytes).map_err(cos_err)?;

    // Read old names first (immutable), then apply (mutable) — no overlap.
    let mut renames: Vec<(ObjectId, Vec<u8>)> = Vec::new();
    for id in acroform_field_ids(&doc)? {
        if let Ok(t) = doc.get_dictionary(id).map_err(cos_err)?.get(b"T").and_then(Object::as_str) {
            let mut new_name = t.to_vec();
            new_name.extend_from_slice(suffix.as_bytes());
            renames.push((id, new_name));
        }
    }
    for (id, new_name) in renames {
        doc.get_dictionary_mut(id)
            .map_err(cos_err)?
            .set("T", Object::string_literal(new_name));
    }

    let mut buf = Vec::new();
    doc.save_to(&mut buf)?;
    Ok(buf)
}

/// Reorder the document's pages by rewriting the root `/Pages` `/Kids` array.
/// `new_order[new_pos] = old_index` (0-based). Object-ref links, bookmarks, and
/// named destinations reference page *objects* (not positions), so they track
/// the move for free — nothing else needs rewriting (P2-PAGE-002).
///
/// Requires a **flat** page tree (the root `/Kids` are exactly the page leaves):
/// errors on a nested tree rather than risk dropping inherited attributes, so
/// the document is never corrupted. Nested-tree reorder is a follow-up.
pub fn reorder_pages(bytes: &[u8], new_order: &[usize]) -> Result<Vec<u8>, CommandError> {
    let mut doc = Document::load_mem(bytes).map_err(cos_err)?;

    let pages_id = doc
        .catalog()
        .map_err(cos_err)?
        .get(b"Pages")
        .and_then(Object::as_reference)
        .map_err(cos_err)?;

    let kids = doc
        .get_dictionary(pages_id)
        .map_err(cos_err)?
        .get(b"Kids")
        .and_then(Object::as_array)
        .map_err(cos_err)?
        .clone();

    // Require a flat page tree: one Kid per page, each a /Page leaf.
    if kids.len() != new_order.len() {
        return Err(CommandError::InvalidInput(format!(
            "reorder needs a flat page tree (Kids={}, pages={}); nested trees unsupported",
            kids.len(),
            new_order.len()
        )));
    }
    for kid in &kids {
        let kid_id = kid.as_reference().map_err(cos_err)?;
        let ty = doc
            .get_dictionary(kid_id)
            .map_err(cos_err)?
            .get(b"Type")
            .ok()
            .and_then(|t| t.as_name().ok());
        if ty != Some(&b"Page"[..]) {
            return Err(CommandError::InvalidInput(
                "reorder needs a flat page tree (a child is not a page)".into(),
            ));
        }
    }

    // Validate the permutation: a bijection of 0..n.
    let n = kids.len();
    let mut seen = vec![false; n];
    for &i in new_order {
        if i >= n || seen[i] {
            return Err(CommandError::InvalidInput("invalid reorder permutation".into()));
        }
        seen[i] = true;
    }

    let new_kids: Vec<Object> = new_order.iter().map(|&i| kids[i].clone()).collect();
    doc.get_dictionary_mut(pages_id)
        .map_err(cos_err)?
        .set("Kids", Object::Array(new_kids));

    let mut buf = Vec::new();
    doc.save_to(&mut buf)?;
    Ok(buf)
}
