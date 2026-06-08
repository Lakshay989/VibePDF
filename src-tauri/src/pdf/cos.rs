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

use std::collections::{BTreeMap, HashMap, HashSet};

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

/// True when `obj` is a dictionary/stream whose `/Type` is `name`.
fn type_is(obj: &Object, name: &[u8]) -> bool {
    obj.type_name().ok() == Some(name)
}

/// Merge `sources` (≥ 2 serialized PDFs) into one, preserving pages, content,
/// annotations, **bookmarks** (one outline subtree per source), and **form
/// fields** (with colliding `/T` names suffixed `_2`, `_3`, …). Returns the
/// merged bytes. SPEC: P2-PAGE-008.
///
/// This is the all-lopdf merge: each source's objects are renumbered to a
/// disjoint id range and combined into one document, then `/Pages`,
/// `/Catalog`, `/Outlines`, and `/AcroForm` are rebuilt. Because the whole
/// object graph is copied (not re-imported page-by-page), nothing is lost.
pub fn merge_documents(sources: &[Vec<u8>]) -> Result<Vec<u8>, CommandError> {
    if sources.len() < 2 {
        return Err(CommandError::InvalidInput(
            "merge needs at least two files".into(),
        ));
    }

    // 1. Load + renumber each source to a disjoint id range; collect every
    //    object, the page ids (in order), and each source's catalog id.
    let mut max_id = 1u32;
    let mut objects: BTreeMap<ObjectId, Object> = BTreeMap::new();
    let mut page_ids: Vec<ObjectId> = Vec::new();
    let mut catalog_ids: Vec<ObjectId> = Vec::new();

    for bytes in sources {
        let mut doc = Document::load_mem(bytes).map_err(cos_err)?;
        doc.renumber_objects_with(max_id);
        max_id = doc.max_id + 1;

        if let Some((cat_id, _)) = doc.objects.iter().find(|(_, o)| type_is(o, b"Catalog")) {
            catalog_ids.push(*cat_id);
        }
        page_ids.extend(doc.get_pages().into_values());
        objects.extend(doc.objects);
    }
    if catalog_ids.is_empty() || page_ids.is_empty() {
        return Err(CommandError::PdfError("a source has no catalog/pages".into()));
    }

    // 2. New merged document. Allocate a fresh /Pages root id, then copy every
    //    object except the structural roots we rebuild; re-parent each page.
    let mut document = Document::with_version("1.5");
    document.max_id = max_id;
    let merged_pages_id = document.new_object_id();

    let page_set: HashSet<ObjectId> = page_ids.iter().copied().collect();
    for (oid, obj) in &objects {
        if page_set.contains(oid) {
            if let Ok(dict) = obj.as_dict() {
                let mut dict = dict.clone();
                dict.set("Parent", Object::Reference(merged_pages_id));
                document.objects.insert(*oid, Object::Dictionary(dict));
            }
            continue;
        }
        // Skip the source Catalog / Pages / Outlines roots — rebuilt below.
        if type_is(obj, b"Catalog") || type_is(obj, b"Pages") || type_is(obj, b"Outlines") {
            continue;
        }
        document.objects.insert(*oid, obj.clone());
    }

    // 3. Merged /Pages root.
    let count = i64::try_from(page_ids.len()).unwrap_or(i64::MAX);
    let mut pages_dict = Dictionary::new();
    pages_dict.set("Type", Object::Name(b"Pages".to_vec()));
    pages_dict.set("Count", Object::Integer(count));
    pages_dict.set(
        "Kids",
        Object::Array(page_ids.iter().map(|&id| Object::Reference(id)).collect()),
    );
    document.objects.insert(merged_pages_id, Object::Dictionary(pages_dict));

    // 4. Merged /Catalog (reuse the first source's, point it at the new roots).
    let merged_catalog_id = catalog_ids[0];
    let mut catalog = objects
        .get(&merged_catalog_id)
        .and_then(|o| o.as_dict().ok())
        .cloned()
        .ok_or_else(|| CommandError::PdfError("first source has no catalog".into()))?;
    catalog.set("Pages", Object::Reference(merged_pages_id));
    catalog.remove(b"Outlines");
    catalog.remove(b"AcroForm");

    if let Some(outlines_id) = merge_outlines(&mut document, &objects, &catalog_ids)? {
        catalog.set("Outlines", Object::Reference(outlines_id));
    }
    if let Some(acroform_id) = merge_acroform(&mut document, &objects, &catalog_ids) {
        catalog.set("AcroForm", Object::Reference(acroform_id));
    }

    document.objects.insert(merged_catalog_id, Object::Dictionary(catalog));
    document.trailer.set("Root", Object::Reference(merged_catalog_id));
    document.max_id = document.objects.keys().map(|&(n, _)| n).max().unwrap_or(0);

    let mut buf = Vec::new();
    document.save_to(&mut buf)?;
    Ok(buf)
}

/// Chain every source's top-level outline items under one new `/Outlines`
/// root. Each item keeps its (renumbered) `/Dest` and nested children; only
/// the top-level `Parent`/`Next`/`Prev` chain is rebuilt. Returns the new
/// root id, or `None` when no source had any bookmarks.
fn merge_outlines(
    document: &mut Document,
    objects: &BTreeMap<ObjectId, Object>,
    catalog_ids: &[ObjectId],
) -> Result<Option<ObjectId>, CommandError> {
    let mut top_items: Vec<ObjectId> = Vec::new();
    for &cat_id in catalog_ids {
        let Some(outlines_id) = objects
            .get(&cat_id)
            .and_then(|o| o.as_dict().ok())
            .and_then(|c| c.get(b"Outlines").ok())
            .and_then(|o| o.as_reference().ok())
        else {
            continue;
        };
        let mut cur = objects
            .get(&outlines_id)
            .and_then(|o| o.as_dict().ok())
            .and_then(|d| d.get(b"First").ok())
            .and_then(|o| o.as_reference().ok());
        while let Some(item_id) = cur {
            top_items.push(item_id);
            cur = objects
                .get(&item_id)
                .and_then(|o| o.as_dict().ok())
                .and_then(|d| d.get(b"Next").ok())
                .and_then(|o| o.as_reference().ok());
        }
    }
    if top_items.is_empty() {
        return Ok(None);
    }

    let mut root = Dictionary::new();
    root.set("Type", Object::Name(b"Outlines".to_vec()));
    root.set("Count", Object::Integer(i64::try_from(top_items.len()).unwrap_or(i64::MAX)));
    root.set("First", Object::Reference(top_items[0]));
    root.set("Last", Object::Reference(top_items[top_items.len() - 1]));
    let root_id = document.add_object(Object::Dictionary(root));

    let last = top_items.len() - 1;
    for i in 0..top_items.len() {
        let item = document.get_dictionary_mut(top_items[i]).map_err(cos_err)?;
        item.set("Parent", Object::Reference(root_id));
        if i > 0 {
            item.set("Prev", Object::Reference(top_items[i - 1]));
        } else {
            item.remove(b"Prev");
        }
        if i < last {
            item.set("Next", Object::Reference(top_items[i + 1]));
        } else {
            item.remove(b"Next");
        }
    }
    Ok(Some(root_id))
}

/// Resolve a catalog's `/AcroForm`, whether it's an indirect reference or an
/// inline dictionary.
fn resolve_acroform<'a>(
    objects: &'a BTreeMap<ObjectId, Object>,
    catalog: &'a Dictionary,
) -> Option<&'a Dictionary> {
    let acro = catalog.get(b"AcroForm").ok()?;
    match acro.as_reference() {
        Ok(id) => objects.get(&id)?.as_dict().ok(),
        Err(_) => acro.as_dict().ok(),
    }
}

/// Merge every source's `/AcroForm` `/Fields` into one form, suffixing
/// colliding top-level field names (`/T`) with `_2`, `_3`, … Returns the new
/// `/AcroForm` object id, or `None` when no source had a form.
fn merge_acroform(
    document: &mut Document,
    objects: &BTreeMap<ObjectId, Object>,
    catalog_ids: &[ObjectId],
) -> Option<ObjectId> {
    let mut field_ids: Vec<ObjectId> = Vec::new();
    let mut default_resources: Option<Dictionary> = None;
    let mut default_appearance: Option<Object> = None;

    for &cat_id in catalog_ids {
        let Some(catalog) = objects.get(&cat_id).and_then(|o| o.as_dict().ok()) else {
            continue;
        };
        let Some(acro) = resolve_acroform(objects, catalog) else {
            continue;
        };
        if let Ok(fields) = acro.get(b"Fields").and_then(Object::as_array) {
            field_ids.extend(fields.iter().filter_map(|f| f.as_reference().ok()));
        }
        if default_appearance.is_none() {
            if let Ok(da) = acro.get(b"DA") {
                default_appearance = Some(da.clone());
            }
        }
        if let Ok(dr) = acro.get(b"DR").and_then(Object::as_dict) {
            let merged = default_resources.get_or_insert_with(Dictionary::new);
            for (k, v) in dr {
                if !merged.has(k) {
                    merged.set(k.clone(), v.clone());
                }
            }
        }
    }
    if field_ids.is_empty() {
        return None;
    }

    // Suffix colliding top-level field names.
    let mut seen: HashMap<Vec<u8>, u32> = HashMap::new();
    for &fid in &field_ids {
        let base = match document.get_dictionary(fid).ok().and_then(|f| f.get(b"T").and_then(Object::as_str).ok()) {
            Some(t) => t.to_vec(),
            None => continue,
        };
        let count = {
            let entry = seen.entry(base.clone()).or_insert(0);
            *entry += 1;
            *entry
        };
        if count > 1 {
            let mut new_name = base;
            new_name.extend_from_slice(format!("_{count}").as_bytes());
            if let Ok(field) = document.get_dictionary_mut(fid) {
                field.set("T", Object::string_literal(new_name));
            }
        }
    }

    let mut acroform = Dictionary::new();
    acroform.set(
        "Fields",
        Object::Array(field_ids.iter().map(|&id| Object::Reference(id)).collect()),
    );
    acroform.set("NeedAppearances", Object::Boolean(true));
    if let Some(da) = default_appearance {
        acroform.set("DA", da);
    }
    if let Some(dr) = default_resources {
        acroform.set("DR", Object::Dictionary(dr));
    }
    Some(document.add_object(Object::Dictionary(acroform)))
}

/// Register the terminal form-field widgets on the pages `[start, start+count)`
/// (0-based) into the document's `/AcroForm`, creating the form if absent and
/// suffixing any `/T` that collides with an existing field name (`name` →
/// `name_2`). Used after an insert-from-PDF to re-attach the inserted pages'
/// form fields, which page import copies as widgets but doesn't link into the
/// form. Terminal fields only (a widget carrying its own `/T`).
///
/// SPEC: P2-PAGE-005 (form fields).
#[allow(clippy::too_many_lines)]
pub fn register_inserted_form_fields(
    bytes: &[u8],
    start: usize,
    count: usize,
) -> Result<Vec<u8>, CommandError> {
    let mut doc = Document::load_mem(bytes).map_err(cos_err)?;

    // 1. Collect terminal widget fields (`/Subtype /Widget` with a `/T`) on the
    //    inserted pages. `get_pages` is keyed by 1-based page number.
    let pages = doc.get_pages();
    let mut new_fields: Vec<ObjectId> = Vec::new();
    for i in start..start.saturating_add(count) {
        let pnum = u32::try_from(i + 1).unwrap_or(u32::MAX);
        let Some(&page_id) = pages.get(&pnum) else {
            continue;
        };
        let annots = doc
            .get_dictionary(page_id)
            .ok()
            .and_then(|p| p.get(b"Annots").and_then(Object::as_array).ok().cloned());
        let Some(annots) = annots else {
            continue;
        };
        for a in &annots {
            let Ok(aid) = a.as_reference() else {
                continue;
            };
            let Ok(annot) = doc.get_dictionary(aid) else {
                continue;
            };
            let is_widget =
                annot.get(b"Subtype").ok().and_then(|s| s.as_name().ok()) == Some(&b"Widget"[..]);
            if is_widget && annot.has(b"T") {
                new_fields.push(aid);
            }
        }
    }
    if new_fields.is_empty() {
        let mut buf = Vec::new();
        doc.save_to(&mut buf)?;
        return Ok(buf);
    }

    // 2. Existing /AcroForm (if any) + the field names already in use.
    let acro_id = doc
        .catalog()
        .map_err(cos_err)?
        .get(b"AcroForm")
        .ok()
        .and_then(|o| o.as_reference().ok());
    let mut existing_fields: Vec<ObjectId> = Vec::new();
    if let Some(aid) = acro_id {
        if let Ok(fields) = doc.get_dictionary(aid).map_err(cos_err)?.get(b"Fields").and_then(Object::as_array) {
            existing_fields = fields.iter().filter_map(|f| f.as_reference().ok()).collect();
        }
    }
    // Idempotent: a widget already in /AcroForm /Fields is not "new" (re-running
    // the pass, or a page whose field is already registered, must be a no-op).
    let existing_set: HashSet<ObjectId> = existing_fields.iter().copied().collect();
    new_fields.retain(|id| !existing_set.contains(id));
    if new_fields.is_empty() {
        let mut buf = Vec::new();
        doc.save_to(&mut buf)?;
        return Ok(buf);
    }
    let mut seen: HashMap<Vec<u8>, u32> = HashMap::new();
    for &fid in &existing_fields {
        if let Some(name) = doc.get_dictionary(fid).ok().and_then(|f| f.get(b"T").and_then(Object::as_str).ok()) {
            *seen.entry(name.to_vec()).or_insert(0) += 1;
        }
    }

    // 3. Suffix colliding new field names.
    for &fid in &new_fields {
        let Some(base) = doc.get_dictionary(fid).ok().and_then(|f| f.get(b"T").and_then(Object::as_str).ok()).map(<[u8]>::to_vec)
        else {
            continue;
        };
        let n = {
            let entry = seen.entry(base.clone()).or_insert(0);
            *entry += 1;
            *entry
        };
        if n > 1 {
            let mut new_name = base;
            new_name.extend_from_slice(format!("_{n}").as_bytes());
            if let Ok(field) = doc.get_dictionary_mut(fid) {
                field.set("T", Object::string_literal(new_name));
            }
        }
    }

    // 4. Append the new fields to /AcroForm /Fields, creating the form if absent.
    let mut all_fields = existing_fields;
    all_fields.extend(new_fields);
    let fields_array =
        Object::Array(all_fields.iter().map(|&id| Object::Reference(id)).collect());
    if let Some(aid) = acro_id {
        let acro = doc.get_dictionary_mut(aid).map_err(cos_err)?;
        acro.set("Fields", fields_array);
        if !acro.has(b"NeedAppearances") {
            acro.set("NeedAppearances", Object::Boolean(true));
        }
    } else {
        let mut acro = Dictionary::new();
        acro.set("Fields", fields_array);
        acro.set("NeedAppearances", Object::Boolean(true));
        let aid = doc.add_object(Object::Dictionary(acro));
        doc.catalog_mut().map_err(cos_err)?.set("AcroForm", Object::Reference(aid));
    }

    let mut buf = Vec::new();
    doc.save_to(&mut buf)?;
    Ok(buf)
}
