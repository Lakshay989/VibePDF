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

use lopdf::{Dictionary, Document, Object, ObjectId, Stream};

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

/// Read a PDF number (`Integer` or `Real`) as `f32`.
#[allow(clippy::cast_precision_loss)] // MediaBox values are small (≤ 14400pt).
fn number_as_f32(obj: &Object) -> Option<f32> {
    match obj {
        Object::Integer(i) => Some(*i as f32),
        Object::Real(r) => Some(*r),
        _ => None,
    }
}

/// The effective `/MediaBox` of `page_id` as `[llx, lly, urx, ury]`, walking up
/// the `/Parent` chain for an inherited box. `None` if absent/malformed.
fn effective_media_box(doc: &Document, page_id: ObjectId) -> Option<[f32; 4]> {
    let mut current = Some(page_id);
    for _ in 0..32 {
        let id = current?;
        let dict = doc.get_dictionary(id).ok()?;
        if let Ok(mb) = dict.get(b"MediaBox").and_then(Object::as_array) {
            if mb.len() == 4 {
                let vals: Option<Vec<f32>> = mb.iter().map(number_as_f32).collect();
                if let Some(v) = vals {
                    return Some([v[0], v[1], v[2], v[3]]);
                }
            }
        }
        current = dict.get(b"Parent").ok().and_then(|o| o.as_reference().ok());
    }
    None
}

/// SPEC: P2-PAGE-010 — resize `pages` (0-based) to `width` × `height` points,
/// scaling each page's content to fit. Done at the COS level rather than via
/// `PDFium`: a page's content is scaled by wrapping its content stream(s) with
/// `q <matrix> cm … Q`, and the `/MediaBox` is set to the new size. (`PDFium`'s
/// page-content transform forces a `reload_in_place` that SIGSEGVs at teardown —
/// see `docs/04`.) When `preserve_aspect` is set, content is scaled uniformly by
/// the smaller ratio and centred; otherwise it is stretched to fill.
///
/// Annotations (`/Annots`) are not re-scaled — their coordinates are left as-is
/// (a documented limitation; see `BACKLOG.md`).
pub fn resize_pages(
    bytes: &[u8],
    pages: &[usize],
    width: f32,
    height: f32,
    preserve_aspect: bool,
) -> Result<Vec<u8>, CommandError> {
    let mut doc = Document::load_mem(bytes).map_err(cos_err)?;
    let page_map = doc.get_pages();
    let page_count = page_map.len();

    for &idx in pages {
        let page_no = u32::try_from(idx)
            .ok()
            .map(|n| n + 1)
            .ok_or_else(|| CommandError::InvalidInput(format!("bad page index: {idx}")))?;
        let page_id = *page_map.get(&page_no).ok_or_else(|| {
            CommandError::InvalidInput(format!(
                "page index out of range: {idx} (document has {page_count} pages)"
            ))
        })?;

        let mb = effective_media_box(&doc, page_id).ok_or_else(|| {
            CommandError::InvalidInput(format!("page {idx} has no readable MediaBox"))
        })?;
        let (ml, mb_bottom, w, h) = (mb[0], mb[1], mb[2] - mb[0], mb[3] - mb[1]);
        if w <= 0.0 || h <= 0.0 {
            return Err(CommandError::InvalidInput(format!(
                "page {idx} has a degenerate MediaBox ({w}×{h})"
            )));
        }

        let (sx, sy, off_x, off_y) = if preserve_aspect {
            let s = (width / w).min(height / h);
            (s, s, (width - w * s) / 2.0, (height - h * s) / 2.0)
        } else {
            (width / w, height / h, 0.0, 0.0)
        };
        // Map the old box origin to the new box: x' = sx*(x - ml) + off_x.
        let e = off_x - sx * ml;
        let f = off_y - sy * mb_bottom;

        // Existing /Contents → a list of stream references (single ref or array).
        let existing: Vec<Object> = {
            let pd = doc.get_dictionary(page_id).map_err(cos_err)?;
            match pd.get(b"Contents") {
                Ok(Object::Reference(id)) => vec![Object::Reference(*id)],
                Ok(Object::Array(arr)) => arr.clone(),
                _ => Vec::new(),
            }
        };

        // Wrap content with `q <scale> cm … Q` (push state, scale, …, pop).
        let pre = format!("q {sx:.6} 0 0 {sy:.6} {e:.6} {f:.6} cm\n").into_bytes();
        let pre_id = doc.add_object(Stream::new(Dictionary::new(), pre));
        let post_id = doc.add_object(Stream::new(Dictionary::new(), b"\nQ".to_vec()));

        let mut contents = Vec::with_capacity(existing.len() + 2);
        contents.push(Object::Reference(pre_id));
        contents.extend(existing);
        contents.push(Object::Reference(post_id));

        let pd = doc.get_dictionary_mut(page_id).map_err(cos_err)?;
        pd.set("Contents", Object::Array(contents));
        pd.set(
            "MediaBox",
            Object::Array(vec![
                Object::Real(0.0),
                Object::Real(0.0),
                Object::Real(width),
                Object::Real(height),
            ]),
        );
        // The old crop/bleed/trim/art boxes describe the pre-resize geometry;
        // drop them so they default to the new MediaBox.
        for key in [&b"CropBox"[..], b"BleedBox", b"TrimBox", b"ArtBox"] {
            pd.remove(key);
        }
    }

    let mut buf = Vec::new();
    doc.save_to(&mut buf)?;
    Ok(buf)
}

/// True when `obj` is a dictionary/stream whose `/Type` is `name`.
fn type_is(obj: &Object, name: &[u8]) -> bool {
    obj.type_name().ok() == Some(name)
}

/// Extract the explicit target page object id from a destination value: a
/// `/Dest` array `[pageRef …]` or an action `<< /S /GoTo /D [pageRef …] >>`.
/// Returns `None` for named destinations (a name/string), indirect dests, or
/// non-`GoTo` actions — those are left untouched.
fn dest_target_page(obj: &Object) -> Option<ObjectId> {
    if let Ok(arr) = obj.as_array() {
        return arr.first().and_then(|o| o.as_reference().ok());
    }
    if let Ok(dict) = obj.as_dict() {
        let is_goto = dict.get(b"S").ok().and_then(|s| s.as_name().ok()) == Some(&b"GoTo"[..]);
        if is_goto {
            if let Ok(d) = dict.get(b"D").and_then(Object::as_array) {
                return d.first().and_then(|o| o.as_reference().ok());
            }
        }
    }
    None
}

/// Whether a `/Link` annotation is broken: a destination-less link (no `/Dest`
/// and no `/A` — e.g. what page-import leaves when the target wasn't copied) or
/// one whose explicit page target no longer exists. A `/URI` (or other non-page
/// action) link, or a named destination we can't resolve, is **kept**.
fn is_broken_link(annot: &Dictionary, page_set: &HashSet<ObjectId>) -> bool {
    let is_link =
        annot.get(b"Subtype").ok().and_then(|s| s.as_name().ok()) == Some(&b"Link"[..]);
    if !is_link {
        return false;
    }
    if !annot.has(b"Dest") && !annot.has(b"A") {
        return true; // dead link: no destination at all
    }
    match nav_target_page(annot) {
        Some(target) => !page_set.contains(&target), // explicit page target is gone
        None => false,                               // named dest / URI action → keep
    }
}

/// The page a navigable dict (link annotation or outline item) targets, via
/// `/Dest` then `/A`. `None` when it has no explicit page destination.
fn nav_target_page(dict: &Dictionary) -> Option<ObjectId> {
    if let Ok(dest) = dict.get(b"Dest") {
        if let Some(id) = dest_target_page(dest) {
            return Some(id);
        }
    }
    if let Ok(action) = dict.get(b"A") {
        if let Some(id) = dest_target_page(action) {
            return Some(id);
        }
    }
    None
}

/// SPEC: P2-PAGE-003 — remove references **to** pages that no longer exist
/// (the other half of "update internal references"). After a delete or split,
/// `/Link` annotations and bookmarks can point at removed pages. This prunes
/// them so the saved file is clean.
///
/// Infallible: returns the input bytes unchanged when nothing dangles (so a
/// clean document is not re-serialized) **or** on any lopdf error (so it can
/// never break saving). Applied on the write path (`save_document`).
#[must_use]
pub fn prune_dangling_destinations(bytes: Vec<u8>) -> Vec<u8> {
    match prune_inner(&bytes) {
        Ok(Some(pruned)) => pruned,
        Ok(None) | Err(_) => bytes,
    }
}

/// Returns `Some(pruned)` when something was removed, `None` when nothing
/// dangled, or an error if the document couldn't be parsed/serialized.
#[allow(clippy::too_many_lines)]
fn prune_inner(bytes: &[u8]) -> Result<Option<Vec<u8>>, CommandError> {
    let mut doc = Document::load_mem(bytes).map_err(cos_err)?;
    let page_set: HashSet<ObjectId> = doc.get_pages().into_values().collect();
    let mut changed = false;

    // 1. Remove dangling /Link annotations from each page's /Annots.
    let page_ids: Vec<ObjectId> = page_set.iter().copied().collect();
    for page_id in page_ids {
        let annots = doc
            .get_dictionary(page_id)
            .ok()
            .and_then(|p| p.get(b"Annots").and_then(Object::as_array).ok().cloned());
        let Some(annots) = annots else {
            continue;
        };
        let mut kept: Vec<Object> = Vec::with_capacity(annots.len());
        let mut removed_any = false;
        for a in &annots {
            let dangling = a
                .as_reference()
                .ok()
                .and_then(|aid| doc.get_dictionary(aid).ok())
                .is_some_and(|annot| is_broken_link(annot, &page_set));
            if dangling {
                removed_any = true;
            } else {
                kept.push(a.clone());
            }
        }
        if removed_any {
            changed = true;
            if let Ok(page) = doc.get_dictionary_mut(page_id) {
                if kept.is_empty() {
                    page.remove(b"Annots");
                } else {
                    page.set("Annots", Object::Array(kept));
                }
            }
        }
    }

    // 2. Bookmarks: drop dangling top-level items (re-chain); neutralize nested.
    if prune_outline(&mut doc, &page_set)? {
        changed = true;
    }

    if !changed {
        return Ok(None);
    }
    // Garbage-collect now-unreferenced objects (the removed link annotations and
    // outline items, plus any leftovers from the page delete) so the file is
    // actually clean, not just functionally correct.
    doc.prune_objects();
    let mut buf = Vec::new();
    doc.save_to(&mut buf)?;
    Ok(Some(buf))
}

/// Remove dangling top-level outline items (re-chaining the survivors) and
/// neutralize dangling nested items (drop their `/Dest`/`/A`). Returns whether
/// anything changed.
fn prune_outline(doc: &mut Document, page_set: &HashSet<ObjectId>) -> Result<bool, CommandError> {
    let Some(outlines_id) = doc
        .catalog()
        .ok()
        .and_then(|c| c.get(b"Outlines").ok())
        .and_then(|o| o.as_reference().ok())
    else {
        return Ok(false);
    };

    // Collect top-level items (the outline root's direct children).
    let mut top: Vec<ObjectId> = Vec::new();
    let mut cur = doc
        .get_dictionary(outlines_id)
        .ok()
        .and_then(|d| d.get(b"First").ok())
        .and_then(|o| o.as_reference().ok());
    while let Some(id) = cur {
        top.push(id);
        cur = doc
            .get_dictionary(id)
            .ok()
            .and_then(|d| d.get(b"Next").ok())
            .and_then(|o| o.as_reference().ok());
    }
    if top.is_empty() {
        return Ok(false);
    }

    let mut survivors: Vec<ObjectId> = Vec::new();
    for &id in &top {
        let dangling = doc
            .get_dictionary(id)
            .ok()
            .and_then(nav_target_page)
            .is_some_and(|t| !page_set.contains(&t));
        if !dangling {
            survivors.push(id);
        }
    }
    let mut changed = survivors.len() != top.len();

    if changed {
        if survivors.is_empty() {
            doc.catalog_mut().map_err(cos_err)?.remove(b"Outlines");
        } else {
            let count = i64::try_from(survivors.len()).unwrap_or(i64::MAX);
            {
                let root = doc.get_dictionary_mut(outlines_id).map_err(cos_err)?;
                root.set("First", Object::Reference(survivors[0]));
                root.set("Last", Object::Reference(survivors[survivors.len() - 1]));
                root.set("Count", Object::Integer(count));
            }
            let last = survivors.len() - 1;
            for i in 0..survivors.len() {
                let item = doc.get_dictionary_mut(survivors[i]).map_err(cos_err)?;
                if i > 0 {
                    item.set("Prev", Object::Reference(survivors[i - 1]));
                } else {
                    item.remove(b"Prev");
                }
                if i < last {
                    item.set("Next", Object::Reference(survivors[i + 1]));
                } else {
                    item.remove(b"Next");
                }
            }
        }
    }

    // Neutralize dangling destinations on nested descendants of the survivors.
    for &s in &survivors {
        if neutralize_dangling_descendants(doc, s, page_set) {
            changed = true;
        }
    }
    Ok(changed)
}

/// Recursively drop `/Dest`/`/A` from descendant outline items whose target
/// page is gone. Returns whether anything changed.
fn neutralize_dangling_descendants(
    doc: &mut Document,
    item_id: ObjectId,
    page_set: &HashSet<ObjectId>,
) -> bool {
    let mut children: Vec<ObjectId> = Vec::new();
    let mut cur = doc
        .get_dictionary(item_id)
        .ok()
        .and_then(|d| d.get(b"First").ok())
        .and_then(|o| o.as_reference().ok());
    while let Some(id) = cur {
        children.push(id);
        cur = doc
            .get_dictionary(id)
            .ok()
            .and_then(|d| d.get(b"Next").ok())
            .and_then(|o| o.as_reference().ok());
    }

    let mut changed = false;
    for child in children {
        let dangling = doc
            .get_dictionary(child)
            .ok()
            .and_then(nav_target_page)
            .is_some_and(|t| !page_set.contains(&t));
        if dangling {
            if let Ok(d) = doc.get_dictionary_mut(child) {
                d.remove(b"Dest");
                d.remove(b"A");
            }
            changed = true;
        }
        if neutralize_dangling_descendants(doc, child, page_set) {
            changed = true;
        }
    }
    changed
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

/// SPEC: P3-ANN-001 — append a text-markup annotation (highlight / underline /
/// strikethrough / squiggly) over `quads` (each `[x1..y4]` in PDF points) on
/// `page` (0-based) to the page's `/Annots`. Writes a standard annotation dict
/// (`/QuadPoints`, `/C`, `/CA`, `/Rect`, `/P`) **plus a generated `/AP`
/// appearance stream** so the markup renders in every reader (`PDFium` can't set
/// annotation colour, so this lives in lopdf).
pub fn add_text_markup(
    bytes: &[u8],
    page: usize,
    subtype: &str,
    quads: &[[f32; 8]],
    color: &str,
    opacity: f32,
) -> Result<Vec<u8>, CommandError> {
    if quads.is_empty() {
        return Err(CommandError::InvalidInput("no quads for text markup".into()));
    }
    let pdf_subtype: &[u8] = match subtype {
        "highlight" => b"Highlight",
        "underline" => b"Underline",
        "strikethrough" => b"StrikeOut",
        "squiggly" => b"Squiggly",
        other => {
            return Err(CommandError::InvalidInput(format!(
                "unknown markup subtype: {other}"
            )))
        }
    };
    let (r, g, b) = parse_hex_color(color)?;
    let opacity = opacity.clamp(0.0, 1.0);

    let mut doc = Document::load_mem(bytes).map_err(cos_err)?;
    let page_no = u32::try_from(page)
        .ok()
        .map(|n| n + 1)
        .ok_or_else(|| CommandError::InvalidInput(format!("bad page index: {page}")))?;
    let page_id = *doc
        .get_pages()
        .get(&page_no)
        .ok_or_else(|| CommandError::InvalidInput(format!("page index out of range: {page}")))?;

    let (x0, y0, x1, y1) = quads_bounds(quads);
    let rect_obj = Object::Array(vec![
        Object::Real(x0),
        Object::Real(y0),
        Object::Real(x1),
        Object::Real(y1),
    ]);

    // Appearance content, drawn in absolute page coords (BBox == Rect, identity
    // matrix, so form space == page space).
    let content = markup_appearance_content(subtype, quads, (r, g, b));

    // Appearance form XObject.
    let mut gs = Dictionary::new();
    gs.set("BM", Object::Name(b"Multiply".to_vec()));
    gs.set("ca", Object::Real(opacity));
    gs.set("CA", Object::Real(opacity));
    let mut ext = Dictionary::new();
    ext.set("GS", Object::Dictionary(gs));
    let mut resources = Dictionary::new();
    resources.set("ExtGState", Object::Dictionary(ext));
    let mut ap_dict = Dictionary::new();
    ap_dict.set("Type", Object::Name(b"XObject".to_vec()));
    ap_dict.set("Subtype", Object::Name(b"Form".to_vec()));
    ap_dict.set("FormType", Object::Integer(1));
    ap_dict.set("BBox", rect_obj.clone());
    ap_dict.set("Resources", Object::Dictionary(resources));
    let ap_id = doc.add_object(Stream::new(ap_dict, content.into_bytes()));

    // /QuadPoints, flattened (UL, UR, LL, LR per quad — the order quads.ts emits).
    let quad_points: Vec<Object> = quads
        .iter()
        .flat_map(|q| q.iter().map(|&v| Object::Real(v)))
        .collect();

    let mut ap = Dictionary::new();
    ap.set("N", Object::Reference(ap_id));
    let mut annot = Dictionary::new();
    annot.set("Type", Object::Name(b"Annot".to_vec()));
    annot.set("Subtype", Object::Name(pdf_subtype.to_vec()));
    annot.set("NM", Object::string_literal(uuid::Uuid::new_v4().to_string())); // stable delete handle
    annot.set("Rect", rect_obj);
    annot.set("QuadPoints", Object::Array(quad_points));
    annot.set("C", Object::Array(vec![Object::Real(r), Object::Real(g), Object::Real(b)]));
    annot.set("CA", Object::Real(opacity));
    annot.set("F", Object::Integer(4)); // Print
    annot.set("P", Object::Reference(page_id));
    annot.set("AP", Object::Dictionary(ap));
    let annot_id = doc.add_object(Object::Dictionary(annot));

    // Append to the page's /Annots (array, indirect array, or absent).
    let existing = doc
        .get_dictionary(page_id)
        .map_err(cos_err)?
        .get(b"Annots")
        .ok()
        .cloned();
    match existing {
        Some(Object::Reference(arr_id)) => {
            if let Ok(Object::Array(arr)) = doc.get_object_mut(arr_id) {
                arr.push(Object::Reference(annot_id));
            } else {
                return Err(CommandError::InvalidInput("malformed /Annots".into()));
            }
        }
        Some(Object::Array(mut arr)) => {
            arr.push(Object::Reference(annot_id));
            doc.get_dictionary_mut(page_id)
                .map_err(cos_err)?
                .set("Annots", Object::Array(arr));
        }
        _ => {
            doc.get_dictionary_mut(page_id)
                .map_err(cos_err)?
                .set("Annots", Object::Array(vec![Object::Reference(annot_id)]));
        }
    }

    let mut buf = Vec::new();
    doc.save_to(&mut buf)?;
    Ok(buf)
}

/// Build the `/AP` appearance content stream for a markup subtype, drawn in
/// absolute page coordinates (the form's `BBox` == `Rect`, identity matrix).
fn markup_appearance_content(subtype: &str, quads: &[[f32; 8]], (r, g, b): (f32, f32, f32)) -> String {
    use std::fmt::Write as _;

    let mut content = String::new();
    if subtype == "highlight" {
        let _ = writeln!(content, "/GS gs");
        let _ = writeln!(content, "{r:.4} {g:.4} {b:.4} rg");
        for q in quads {
            let (qx0, qy0, qx1, qy1) = quad_bbox(q);
            let _ = writeln!(content, "{qx0:.2} {qy0:.2} {:.2} {:.2} re f", qx1 - qx0, qy1 - qy0);
        }
        return content;
    }

    let _ = writeln!(content, "{r:.4} {g:.4} {b:.4} RG");
    for q in quads {
        let (qx0, qy0, qx1, qy1) = quad_bbox(q);
        let line_w = ((qy1 - qy0) * 0.06).max(0.75);
        let _ = writeln!(content, "{line_w:.2} w");
        let y = if subtype == "strikethrough" {
            (qy0 + qy1) / 2.0
        } else {
            qy0 + (qy1 - qy0) * 0.12
        };
        if subtype == "squiggly" {
            write_squiggle(&mut content, qx0, qx1, y, (qy1 - qy0) * 0.12);
        } else {
            let _ = writeln!(content, "{qx0:.2} {y:.2} m {qx1:.2} {y:.2} l S");
        }
    }
    content
}

/// SPEC: P3-ANN-003 — add a free-text annotation: a `/FreeText` box at `rect`
/// holding `text` in a base-14 font, plus a generated `/AP` appearance so it
/// renders in every reader (`PDFium` can't author this). One uniform style
/// (family / size / colour / bold / italic); underline + rich text are B3b.
#[allow(clippy::too_many_arguments)]
pub fn add_free_text(
    bytes: &[u8],
    page: usize,
    rect: [f32; 4],
    text: &str,
    font_family: &str,
    font_size: f32,
    color: &str,
    bold: bool,
    italic: bool,
) -> Result<Vec<u8>, CommandError> {
    let (r, g, b) = parse_hex_color(color)?;
    let base = base_font(font_family, bold, italic)?;
    let size = font_size.max(1.0);
    let [x0, y0, x1, y1] = rect;
    if !(x1 > x0 && y1 > y0) {
        return Err(CommandError::InvalidInput("free-text rect is empty".into()));
    }
    let rect = grow_free_text_rect(rect, text, size);

    let mut doc = Document::load_mem(bytes).map_err(cos_err)?;
    let page_no = u32::try_from(page)
        .ok()
        .map(|n| n + 1)
        .ok_or_else(|| CommandError::InvalidInput(format!("bad page index: {page}")))?;
    let page_id = *doc
        .get_pages()
        .get(&page_no)
        .ok_or_else(|| CommandError::InvalidInput(format!("page index out of range: {page}")))?;

    let (ap, da) = free_text_appearance(&mut doc, rect, text, base, size, (r, g, b));

    let mut annot = Dictionary::new();
    annot.set("Type", Object::Name(b"Annot".to_vec()));
    annot.set("Subtype", Object::Name(b"FreeText".to_vec()));
    annot.set("NM", Object::string_literal(uuid::Uuid::new_v4().to_string())); // stable handle
    annot.set("Rect", rect_array(rect));
    annot.set("Contents", Object::string_literal(text));
    annot.set("DA", Object::string_literal(da));
    annot.set("F", Object::Integer(4)); // Print
    annot.set("P", Object::Reference(page_id));
    annot.set("AP", Object::Dictionary(ap));
    let annot_id = doc.add_object(Object::Dictionary(annot));

    append_annotation(&mut doc, page_id, annot_id)?;
    let mut buf = Vec::new();
    doc.save_to(&mut buf)?;
    Ok(buf)
}

/// SPEC: P3-ANN-013 — update an existing free-text annotation (found by `/NM`):
/// new `text` + style, rewriting `/Contents` + `/Rect` (grown to fit) + `/DA` +
/// `/AP` while preserving the `/NM` and every other field. The old `/AP` stream
/// is GC'd by `prune_objects`.
#[allow(clippy::too_many_arguments)]
pub fn update_free_text(
    bytes: &[u8],
    nm: &str,
    text: &str,
    font_family: &str,
    font_size: f32,
    color: &str,
    bold: bool,
    italic: bool,
) -> Result<Vec<u8>, CommandError> {
    let (r, g, b) = parse_hex_color(color)?;
    let base = base_font(font_family, bold, italic)?;
    let size = font_size.max(1.0);

    let mut doc = Document::load_mem(bytes).map_err(cos_err)?;
    let id = find_annotation_by_nm(&doc, nm)
        .ok_or_else(|| CommandError::InvalidInput(format!("free-text not found: {nm}")))?;
    let dict = doc.get_dictionary(id).map_err(cos_err)?;
    if dict.get(b"Subtype").and_then(Object::as_name).ok() != Some(&b"FreeText"[..]) {
        return Err(CommandError::InvalidInput("annotation is not free-text".into()));
    }
    // Keep the top edge + width; grow only downward to fit the new text.
    let rect = grow_free_text_rect(rect_bounds(dict), text, size);

    let (ap, da) = free_text_appearance(&mut doc, rect, text, base, size, (r, g, b));
    let dict = doc.get_dictionary_mut(id).map_err(cos_err)?;
    dict.set("Contents", Object::string_literal(text));
    dict.set("Rect", rect_array(rect));
    dict.set("DA", Object::string_literal(da));
    dict.set("AP", Object::Dictionary(ap));

    doc.prune_objects(); // drop the now-orphaned old /AP stream
    let mut buf = Vec::new();
    doc.save_to(&mut buf)?;
    Ok(buf)
}

/// A free-text annotation's editable state, read back for the in-place editor.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FreeTextData {
    pub rect: [f32; 4],
    pub text: String,
    pub font_family: String,
    pub font_size: f32,
    pub color: String,
    pub bold: bool,
    pub italic: bool,
}

/// SPEC: P3-ANN-013 — read a free-text annotation's text + style by `/NM`, so the
/// editor can open pre-filled. `None` if there's no such free-text. Style is
/// parsed from the `/DA` (size + colour) and the `/AP` font `/BaseFont` (family +
/// bold/italic); a foreign annotation that doesn't match our format falls back to
/// defaults.
pub fn read_free_text(bytes: &[u8], nm: &str) -> Result<Option<FreeTextData>, CommandError> {
    let doc = Document::load_mem(bytes).map_err(cos_err)?;
    let Some(id) = find_annotation_by_nm(&doc, nm) else { return Ok(None) };
    let Ok(dict) = doc.get_dictionary(id) else { return Ok(None) };
    if dict.get(b"Subtype").and_then(Object::as_name).ok() != Some(&b"FreeText"[..]) {
        return Ok(None);
    }
    let da = dict
        .get(b"DA")
        .and_then(Object::as_str)
        .ok()
        .map_or_else(String::new, |s| String::from_utf8_lossy(s).into_owned());
    let (font_size, color) = parse_da(&da);
    let (font_family, bold, italic) = font_from_base(read_ap_base_font(&doc, dict).as_deref());
    Ok(Some(FreeTextData {
        rect: rect_bounds(dict),
        text: str_field(dict, b"Contents"),
        font_family,
        font_size,
        color,
        bold,
        italic,
    }))
}

/// `[x0,y0,x1,y1]` → a PDF `/Rect` array.
fn rect_array(rect: [f32; 4]) -> Object {
    Object::Array(rect.iter().map(|&v| Object::Real(v)).collect())
}

/// Grow `rect` downward (top edge + width fixed) so `text` at `size` fits the
/// `/AP` box (which clips to `BBox == Rect`). Never shrinks below the input.
fn grow_free_text_rect(rect: [f32; 4], text: &str, size: f32) -> [f32; 4] {
    let [x0, y0_in, x1, y1] = rect;
    let leading = size * 1.2;
    let line_count = u16::try_from(text.split('\n').count().max(1)).unwrap_or(u16::MAX);
    let needed = f32::from(line_count) * leading + size * 0.35;
    [x0, (y1 - needed).min(y0_in), x1, y1]
}

/// Build the free-text `/AP` form (added to `doc`) + its `/DA` string for the
/// given geometry/style. Shared by [`add_free_text`] and [`update_free_text`].
fn free_text_appearance(
    doc: &mut Document,
    rect: [f32; 4],
    text: &str,
    base: &str,
    size: f32,
    (r, g, b): (f32, f32, f32),
) -> (Dictionary, String) {
    let content = free_text_appearance_content(rect, text, size, (r, g, b));

    // The appearance's own font resource — `/AP` is self-contained, so display
    // doesn't depend on an AcroForm `/DR`.
    let mut font = Dictionary::new();
    font.set("Type", Object::Name(b"Font".to_vec()));
    font.set("Subtype", Object::Name(b"Type1".to_vec()));
    font.set("BaseFont", Object::Name(base.as_bytes().to_vec()));
    let mut fonts = Dictionary::new();
    fonts.set("F1", Object::Dictionary(font));
    let mut resources = Dictionary::new();
    resources.set("Font", Object::Dictionary(fonts));

    let mut ap_dict = Dictionary::new();
    ap_dict.set("Type", Object::Name(b"XObject".to_vec()));
    ap_dict.set("Subtype", Object::Name(b"Form".to_vec()));
    ap_dict.set("FormType", Object::Integer(1));
    ap_dict.set("BBox", rect_array(rect));
    ap_dict.set("Resources", Object::Dictionary(resources));
    let ap_id = doc.add_object(Stream::new(ap_dict, content.into_bytes()));

    let mut ap = Dictionary::new();
    ap.set("N", Object::Reference(ap_id));
    // `/DA` (default appearance) — best-effort fallback for a reader that
    // regenerates appearance instead of using `/AP`.
    let da = format!("/F1 {size:.2} Tf {r:.4} {g:.4} {b:.4} rg");
    (ap, da)
}

/// Parse a free-text `/DA` (`/F1 <size> Tf <r> <g> <b> rg`) into `(size, #hex)`.
/// Defaults on anything we don't recognize.
fn parse_da(da: &str) -> (f32, String) {
    let toks: Vec<&str> = da.split_whitespace().collect();
    let size = toks
        .iter()
        .position(|t| *t == "Tf")
        .and_then(|i| i.checked_sub(1))
        .and_then(|i| toks.get(i))
        .and_then(|t| t.parse::<f32>().ok())
        .unwrap_or(14.0);
    let color = toks
        .iter()
        .position(|t| *t == "rg")
        .filter(|&i| i >= 3)
        .and_then(|i| {
            let r = toks[i - 3].parse::<f32>().ok()?;
            let g = toks[i - 2].parse::<f32>().ok()?;
            let b = toks[i - 1].parse::<f32>().ok()?;
            Some(rgb_to_hex(r, g, b))
        })
        .unwrap_or_else(|| "#000000".to_string());
    (size, color)
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // 0..=255 after clamp
fn rgb_to_hex(r: f32, g: f32, b: f32) -> String {
    let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", c(r), c(g), c(b))
}

/// The `/BaseFont` of a free-text annotation's `/AP` font resource, if any.
fn read_ap_base_font(doc: &Document, annot: &Dictionary) -> Option<String> {
    let n = annot.get(b"AP").and_then(Object::as_dict).ok()?.get(b"N").ok()?.as_reference().ok()?;
    let stream = doc.get_object(n).and_then(Object::as_stream).ok()?;
    let base = stream
        .dict
        .get(b"Resources")
        .and_then(Object::as_dict)
        .ok()?
        .get(b"Font")
        .and_then(Object::as_dict)
        .ok()?
        .get(b"F1")
        .and_then(Object::as_dict)
        .ok()?
        .get(b"BaseFont")
        .and_then(Object::as_name)
        .ok()?;
    Some(String::from_utf8_lossy(base).into_owned())
}

/// Inverse of [`base_font`]: a base-14 PostScript name → `(family, bold, italic)`.
/// Unknown names fall back to Helvetica regular.
fn font_from_base(base: Option<&str>) -> (String, bool, bool) {
    let (family, bold, italic) = match base.unwrap_or("Helvetica") {
        "Helvetica-Bold" => ("Helvetica", true, false),
        "Helvetica-Oblique" => ("Helvetica", false, true),
        "Helvetica-BoldOblique" => ("Helvetica", true, true),
        "Times-Roman" => ("Times", false, false),
        "Times-Bold" => ("Times", true, false),
        "Times-Italic" => ("Times", false, true),
        "Times-BoldItalic" => ("Times", true, true),
        "Courier" => ("Courier", false, false),
        "Courier-Bold" => ("Courier", true, false),
        "Courier-Oblique" => ("Courier", false, true),
        "Courier-BoldOblique" => ("Courier", true, true),
        _ => ("Helvetica", false, false),
    };
    (family.to_string(), bold, italic)
}

/// The `/AP` content stream for a free-text box: each line of `text` drawn
/// top-anchored inside `rect`, inset 2pt, with 1.2×size leading. Honors explicit
/// newlines only — auto-wrap is B3b.
fn free_text_appearance_content(
    rect: [f32; 4],
    text: &str,
    size: f32,
    (r, g, b): (f32, f32, f32),
) -> String {
    use std::fmt::Write as _;
    let [x0, _y0, _x1, y1] = rect;
    let leading = size * 1.2;
    let tx = x0 + 2.0; // small left inset
    let y_top = y1 - size; // first baseline a little below the top edge

    let mut out = String::new();
    let _ = writeln!(out, "q");
    let _ = writeln!(out, "BT");
    let _ = writeln!(out, "/F1 {size:.2} Tf");
    let _ = writeln!(out, "{leading:.2} TL");
    let _ = writeln!(out, "{r:.4} {g:.4} {b:.4} rg");
    let _ = writeln!(out, "{tx:.2} {y_top:.2} Td");
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            let _ = writeln!(out, "T*");
        }
        let _ = writeln!(out, "({}) Tj", pdf_escape(line));
    }
    let _ = writeln!(out, "ET");
    let _ = writeln!(out, "Q");
    out
}

/// Map a UI font family + bold/italic to its base-14 PostScript name.
fn base_font(family: &str, bold: bool, italic: bool) -> Result<&'static str, CommandError> {
    let name = match (family, bold, italic) {
        ("Helvetica", false, false) => "Helvetica",
        ("Helvetica", true, false) => "Helvetica-Bold",
        ("Helvetica", false, true) => "Helvetica-Oblique",
        ("Helvetica", true, true) => "Helvetica-BoldOblique",
        ("Times", false, false) => "Times-Roman",
        ("Times", true, false) => "Times-Bold",
        ("Times", false, true) => "Times-Italic",
        ("Times", true, true) => "Times-BoldItalic",
        ("Courier", false, false) => "Courier",
        ("Courier", true, false) => "Courier-Bold",
        ("Courier", false, true) => "Courier-Oblique",
        ("Courier", true, true) => "Courier-BoldOblique",
        (other, _, _) => {
            return Err(CommandError::InvalidInput(format!("unknown font family: {other}")))
        }
    };
    Ok(name)
}

/// Escape a PDF literal-string body (`\`, `(`, `)`). Non-ASCII passes through —
/// base-14 fonts only render the ASCII/WinAnsi range (documented limit).
fn pdf_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '(' => out.push_str("\\("),
            ')' => out.push_str("\\)"),
            c => out.push(c),
        }
    }
    out
}

/// Average Helvetica-Bold glyph advance (em) — used to auto-fit + centre a
/// stamp's single line of uppercase text. Exact metrics aren't needed.
const STAMP_GLYPH_EM: f32 = 0.62;

/// SPEC: P3-ANN-006 — add a `/Stamp` annotation: a rubber-stamp box (a coloured
/// border with the bold uppercase `text` auto-fit + centred inside `rect`), with
/// a generated `/AP` so it renders identically in every reader. `name` is the
/// (informational) PDF `/Name`; `text` is the visible label + `/Contents`.
/// Image stamps are C3b.
pub fn add_stamp(
    bytes: &[u8],
    page: usize,
    rect: [f32; 4],
    text: &str,
    name: &str,
    color: &str,
    opacity: f32,
) -> Result<Vec<u8>, CommandError> {
    let label = text.trim();
    if label.is_empty() {
        return Err(CommandError::InvalidInput("stamp text is empty".into()));
    }
    let [x0, y0, x1, y1] = rect;
    if !(x1 > x0 && y1 > y0) {
        return Err(CommandError::InvalidInput("stamp rect is empty".into()));
    }
    let (r, g, b) = parse_hex_color(color)?;
    let opacity = opacity.clamp(0.0, 1.0);

    let mut doc = Document::load_mem(bytes).map_err(cos_err)?;
    let page_no = u32::try_from(page)
        .ok()
        .map(|n| n + 1)
        .ok_or_else(|| CommandError::InvalidInput(format!("bad page index: {page}")))?;
    let page_id = *doc
        .get_pages()
        .get(&page_no)
        .ok_or_else(|| CommandError::InvalidInput(format!("page index out of range: {page}")))?;

    let content = stamp_appearance_content(rect, label, (r, g, b));

    // `/AP` resources: an ExtGState for the stamp's opacity + the bold base-14
    // font the label is drawn with (self-contained, no AcroForm `/DR` needed).
    let mut gs = Dictionary::new();
    gs.set("ca", Object::Real(opacity));
    gs.set("CA", Object::Real(opacity));
    let mut ext = Dictionary::new();
    ext.set("GS", Object::Dictionary(gs));
    let mut font = Dictionary::new();
    font.set("Type", Object::Name(b"Font".to_vec()));
    font.set("Subtype", Object::Name(b"Type1".to_vec()));
    font.set("BaseFont", Object::Name(b"Helvetica-Bold".to_vec()));
    let mut fonts = Dictionary::new();
    fonts.set("F1", Object::Dictionary(font));
    let mut resources = Dictionary::new();
    resources.set("ExtGState", Object::Dictionary(ext));
    resources.set("Font", Object::Dictionary(fonts));

    let mut ap_dict = Dictionary::new();
    ap_dict.set("Type", Object::Name(b"XObject".to_vec()));
    ap_dict.set("Subtype", Object::Name(b"Form".to_vec()));
    ap_dict.set("FormType", Object::Integer(1));
    ap_dict.set("BBox", rect_array(rect));
    ap_dict.set("Resources", Object::Dictionary(resources));
    let ap_id = doc.add_object(Stream::new(ap_dict, content.into_bytes()));
    let mut ap = Dictionary::new();
    ap.set("N", Object::Reference(ap_id));

    let mut annot = Dictionary::new();
    annot.set("Type", Object::Name(b"Annot".to_vec()));
    annot.set("Subtype", Object::Name(b"Stamp".to_vec()));
    annot.set("NM", Object::string_literal(uuid::Uuid::new_v4().to_string())); // stable handle
    annot.set("Name", Object::Name(sanitize_stamp_name(name).into_bytes()));
    annot.set("Rect", rect_array(rect));
    annot.set("Contents", Object::string_literal(label));
    annot.set("C", Object::Array(vec![Object::Real(r), Object::Real(g), Object::Real(b)]));
    annot.set("CA", Object::Real(opacity));
    annot.set("F", Object::Integer(4)); // Print
    annot.set("P", Object::Reference(page_id));
    annot.set("AP", Object::Dictionary(ap));
    let annot_id = doc.add_object(Object::Dictionary(annot));

    append_annotation(&mut doc, page_id, annot_id)?;
    let mut buf = Vec::new();
    doc.save_to(&mut buf)?;
    Ok(buf)
}

/// A PDF `/Name` token for a stamp — ASCII alphanumerics only (drop spaces and
/// punctuation); falls back to `Draft` when nothing survives. Informational: the
/// `/AP` is what actually renders.
fn sanitize_stamp_name(name: &str) -> String {
    let cleaned: String = name.chars().filter(char::is_ascii_alphanumeric).collect();
    if cleaned.is_empty() {
        "Draft".to_string()
    } else {
        cleaned
    }
}

/// The `/AP` content stream for a stamp: a coloured border inset from the box,
/// with the bold uppercase `label` auto-sized to fit and centred. Absolute page
/// coords (`BBox == Rect`). Width is estimated with a Helvetica-Bold average
/// glyph advance — exact metrics aren't needed to centre one line.
fn stamp_appearance_content(rect: [f32; 4], label: &str, color: (f32, f32, f32)) -> String {
    use std::fmt::Write as _;
    let (cr, cg, cb) = color;
    let [x0, y0, x1, y1] = rect;
    let bw = x1 - x0;
    let bh = y1 - y0;
    let pad = (bh * 0.12).clamp(3.0, 8.0); // border inset
    let border_w = (bh * 0.04).clamp(1.0, 3.0);

    let upper = label.to_uppercase();
    let count = u16::try_from(upper.chars().count().max(1)).unwrap_or(u16::MAX);
    let len = f32::from(count);
    let text_pad = pad + 4.0;
    let by_width = (bw - 2.0 * text_pad) / (STAMP_GLYPH_EM * len);
    let by_height = (bh - 2.0 * pad) * 0.62;
    let size = by_width.min(by_height).clamp(5.0, 96.0);
    let text_w = STAMP_GLYPH_EM * size * len;
    let tx = x0 + (bw - text_w) / 2.0;
    let baseline = (y0 + y1) / 2.0 - size * 0.34; // centre the cap height

    let (inx, iny, inw, inh) = (x0 + pad, y0 + pad, bw - 2.0 * pad, bh - 2.0 * pad);

    let mut out = String::new();
    let _ = writeln!(out, "/GS gs");
    let _ = writeln!(out, "{border_w:.2} w");
    let _ = writeln!(out, "{cr:.4} {cg:.4} {cb:.4} RG");
    let _ = writeln!(out, "{cr:.4} {cg:.4} {cb:.4} rg");
    let _ = writeln!(out, "{inx:.2} {iny:.2} {inw:.2} {inh:.2} re");
    let _ = writeln!(out, "S");
    let _ = writeln!(out, "BT");
    let _ = writeln!(out, "/F1 {size:.2} Tf");
    let _ = writeln!(out, "{tx:.2} {baseline:.2} Td");
    let _ = writeln!(out, "({}) Tj", pdf_escape(&upper));
    let _ = writeln!(out, "ET");
    out
}

/// SPEC: P3-ANN-004 — add a shape annotation: `/Square` for a rectangle or
/// `/Circle` for an ellipse, bounded by `rect`, with a generated `/AP` so it
/// renders in every reader (`PDFium` can't author a coloured shape). Stroke +
/// optional fill + opacity + border width. Lines/arrows/polygons are C1b.
#[allow(clippy::too_many_arguments)]
pub fn add_shape(
    bytes: &[u8],
    page: usize,
    kind: &str,
    rect: [f32; 4],
    stroke: &str,
    fill: Option<&str>,
    opacity: f32,
    stroke_width: f32,
) -> Result<Vec<u8>, CommandError> {
    let subtype: &[u8] = match kind {
        "rectangle" => b"Square",
        "ellipse" => b"Circle",
        other => return Err(CommandError::InvalidInput(format!("unknown shape kind: {other}"))),
    };
    let (sr, sg, sb) = parse_hex_color(stroke)?;
    let fill_rgb = fill.map(parse_hex_color).transpose()?;
    let opacity = opacity.clamp(0.0, 1.0);
    let width = stroke_width.max(0.0);
    let [x0, y0, x1, y1] = rect;
    if !(x1 > x0 && y1 > y0) {
        return Err(CommandError::InvalidInput("shape rect is empty".into()));
    }

    let mut doc = Document::load_mem(bytes).map_err(cos_err)?;
    let page_no = u32::try_from(page)
        .ok()
        .map(|n| n + 1)
        .ok_or_else(|| CommandError::InvalidInput(format!("bad page index: {page}")))?;
    let page_id = *doc
        .get_pages()
        .get(&page_no)
        .ok_or_else(|| CommandError::InvalidInput(format!("page index out of range: {page}")))?;

    let rect_obj =
        Object::Array(vec![Object::Real(x0), Object::Real(y0), Object::Real(x1), Object::Real(y1)]);

    let content = shape_appearance_content(kind, rect, (sr, sg, sb), fill_rgb, width);

    // Opacity lives on an ExtGState shared by stroke + fill in the appearance.
    let mut gs = Dictionary::new();
    gs.set("ca", Object::Real(opacity));
    gs.set("CA", Object::Real(opacity));
    let mut ext = Dictionary::new();
    ext.set("GS", Object::Dictionary(gs));
    let mut resources = Dictionary::new();
    resources.set("ExtGState", Object::Dictionary(ext));

    let mut ap_dict = Dictionary::new();
    ap_dict.set("Type", Object::Name(b"XObject".to_vec()));
    ap_dict.set("Subtype", Object::Name(b"Form".to_vec()));
    ap_dict.set("FormType", Object::Integer(1));
    ap_dict.set("BBox", rect_obj.clone());
    ap_dict.set("Resources", Object::Dictionary(resources));
    let ap_id = doc.add_object(Stream::new(ap_dict, content.into_bytes()));

    let mut ap = Dictionary::new();
    ap.set("N", Object::Reference(ap_id));

    let mut bs = Dictionary::new();
    bs.set("W", Object::Real(width));
    bs.set("S", Object::Name(b"S".to_vec())); // solid border

    let mut annot = Dictionary::new();
    annot.set("Type", Object::Name(b"Annot".to_vec()));
    annot.set("Subtype", Object::Name(subtype.to_vec()));
    annot.set("NM", Object::string_literal(uuid::Uuid::new_v4().to_string())); // stable delete handle
    annot.set("Rect", rect_obj);
    annot.set("C", Object::Array(vec![Object::Real(sr), Object::Real(sg), Object::Real(sb)]));
    if let Some((fr, fg, fb)) = fill_rgb {
        annot.set("IC", Object::Array(vec![Object::Real(fr), Object::Real(fg), Object::Real(fb)]));
    }
    annot.set("CA", Object::Real(opacity));
    annot.set("BS", Object::Dictionary(bs));
    annot.set("F", Object::Integer(4)); // Print
    annot.set("P", Object::Reference(page_id));
    annot.set("AP", Object::Dictionary(ap));
    let annot_id = doc.add_object(Object::Dictionary(annot));

    append_annotation(&mut doc, page_id, annot_id)?;
    let mut buf = Vec::new();
    doc.save_to(&mut buf)?;
    Ok(buf)
}

/// SPEC: P3-ANN-004 — add a line annotation from `(x1,y1)` to `(x2,y2)` with a
/// generated `/AP`. `arrow` adds an `/LE` open-arrow ending + an arrowhead in the
/// appearance. Stroke colour / opacity / width; no fill. Polygons are C1b₂.
#[allow(clippy::too_many_arguments)]
pub fn add_line(
    bytes: &[u8],
    page: usize,
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    arrow: bool,
    stroke: &str,
    opacity: f32,
    stroke_width: f32,
) -> Result<Vec<u8>, CommandError> {
    let (sr, sg, sb) = parse_hex_color(stroke)?;
    let opacity = opacity.clamp(0.0, 1.0);
    let width = stroke_width.max(0.0);
    if (x2 - x1).hypot(y2 - y1) < 1.0 {
        return Err(CommandError::InvalidInput("line is zero-length".into()));
    }

    let mut doc = Document::load_mem(bytes).map_err(cos_err)?;
    let page_no = u32::try_from(page)
        .ok()
        .map(|n| n + 1)
        .ok_or_else(|| CommandError::InvalidInput(format!("bad page index: {page}")))?;
    let page_id = *doc
        .get_pages()
        .get(&page_no)
        .ok_or_else(|| CommandError::InvalidInput(format!("page index out of range: {page}")))?;

    let head = if arrow { arrowhead_points(x1, y1, x2, y2, width) } else { None };

    // `/BBox` must cover the segment + the arrowhead + half the stroke width
    // (the `/AP` form clips to it).
    let pad = width.max(1.0);
    let mut xs = vec![x1, x2];
    let mut ys = vec![y1, y2];
    if let Some((lft, rgt)) = head {
        xs.push(lft.0);
        xs.push(rgt.0);
        ys.push(lft.1);
        ys.push(rgt.1);
    }
    let bx0 = xs.iter().copied().fold(f32::INFINITY, f32::min) - pad;
    let by0 = ys.iter().copied().fold(f32::INFINITY, f32::min) - pad;
    let bx1 = xs.iter().copied().fold(f32::NEG_INFINITY, f32::max) + pad;
    let by1 = ys.iter().copied().fold(f32::NEG_INFINITY, f32::max) + pad;
    let rect_obj =
        Object::Array(vec![Object::Real(bx0), Object::Real(by0), Object::Real(bx1), Object::Real(by1)]);

    let content = line_appearance_content(x1, y1, x2, y2, head, (sr, sg, sb), width);

    let mut gs = Dictionary::new();
    gs.set("ca", Object::Real(opacity));
    gs.set("CA", Object::Real(opacity));
    let mut ext = Dictionary::new();
    ext.set("GS", Object::Dictionary(gs));
    let mut resources = Dictionary::new();
    resources.set("ExtGState", Object::Dictionary(ext));

    let mut ap_dict = Dictionary::new();
    ap_dict.set("Type", Object::Name(b"XObject".to_vec()));
    ap_dict.set("Subtype", Object::Name(b"Form".to_vec()));
    ap_dict.set("FormType", Object::Integer(1));
    ap_dict.set("BBox", rect_obj.clone());
    ap_dict.set("Resources", Object::Dictionary(resources));
    let ap_id = doc.add_object(Stream::new(ap_dict, content.into_bytes()));
    let mut ap = Dictionary::new();
    ap.set("N", Object::Reference(ap_id));

    let mut bs = Dictionary::new();
    bs.set("W", Object::Real(width));
    bs.set("S", Object::Name(b"S".to_vec()));

    let mut annot = Dictionary::new();
    annot.set("Type", Object::Name(b"Annot".to_vec()));
    annot.set("Subtype", Object::Name(b"Line".to_vec()));
    annot.set("NM", Object::string_literal(uuid::Uuid::new_v4().to_string()));
    annot.set(
        "L",
        Object::Array(vec![Object::Real(x1), Object::Real(y1), Object::Real(x2), Object::Real(y2)]),
    );
    annot.set("Rect", rect_obj);
    annot.set("C", Object::Array(vec![Object::Real(sr), Object::Real(sg), Object::Real(sb)]));
    annot.set("CA", Object::Real(opacity));
    annot.set("BS", Object::Dictionary(bs));
    if arrow {
        annot.set(
            "LE",
            Object::Array(vec![Object::Name(b"None".to_vec()), Object::Name(b"OpenArrow".to_vec())]),
        );
    }
    annot.set("F", Object::Integer(4)); // Print
    annot.set("P", Object::Reference(page_id));
    annot.set("AP", Object::Dictionary(ap));
    let annot_id = doc.add_object(Object::Dictionary(annot));

    append_annotation(&mut doc, page_id, annot_id)?;
    let mut buf = Vec::new();
    doc.save_to(&mut buf)?;
    Ok(buf)
}

/// The two base corners of an open-arrow head at `(x2,y2)`, aimed back along the
/// segment from `(x1,y1)`. `None` for a degenerate (zero-length) segment.
fn arrowhead_points(x1: f32, y1: f32, x2: f32, y2: f32, width: f32) -> Option<((f32, f32), (f32, f32))> {
    let (dx, dy) = (x2 - x1, y2 - y1);
    let len = dx.hypot(dy);
    if len < 1.0 {
        return None;
    }
    let (ux, uy) = (dx / len, dy / len);
    let head_len = (width * 3.0).max(8.0);
    let head_w = head_len * 0.9;
    let (bx, by) = (x2 - ux * head_len, y2 - uy * head_len);
    let (px, py) = (-uy, ux); // unit perpendicular
    let lft = (bx + px * head_w / 2.0, by + py * head_w / 2.0);
    let rgt = (bx - px * head_w / 2.0, by - py * head_w / 2.0);
    Some((lft, rgt))
}

/// The `/AP` content stream for a line: stroke the segment, then (if `head` is
/// set) stroke an open-arrow `left → end → right` V. Absolute page coords.
fn line_appearance_content(
    x1: f32,
    y1: f32,
    x2: f32,
    y2: f32,
    head: Option<((f32, f32), (f32, f32))>,
    (sr, sg, sb): (f32, f32, f32),
    width: f32,
) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(out, "/GS gs");
    let _ = writeln!(out, "{width:.2} w");
    let _ = writeln!(out, "{sr:.4} {sg:.4} {sb:.4} RG");
    let _ = writeln!(out, "{x1:.2} {y1:.2} m");
    let _ = writeln!(out, "{x2:.2} {y2:.2} l");
    let _ = writeln!(out, "S");
    if let Some((lft, rgt)) = head {
        let _ = writeln!(out, "{:.2} {:.2} m", lft.0, lft.1);
        let _ = writeln!(out, "{x2:.2} {y2:.2} l");
        let _ = writeln!(out, "{:.2} {:.2} l", rgt.0, rgt.1);
        let _ = writeln!(out, "S");
    }
    out
}

/// SPEC: P3-ANN-004 — add a polygon (`closed`, `/Polygon`) or polyline (`!closed`,
/// `/PolyLine`) through `points`, with a generated `/AP`. A closed polygon can be
/// filled; a polyline is stroke-only. Stroke colour / opacity / width.
#[allow(clippy::too_many_arguments)]
pub fn add_polygon(
    bytes: &[u8],
    page: usize,
    closed: bool,
    points: &[[f32; 2]],
    stroke: &str,
    fill: Option<&str>,
    opacity: f32,
    stroke_width: f32,
) -> Result<Vec<u8>, CommandError> {
    let min_pts = if closed { 3 } else { 2 };
    if points.len() < min_pts {
        return Err(CommandError::InvalidInput(format!("needs at least {min_pts} points")));
    }
    let (sr, sg, sb) = parse_hex_color(stroke)?;
    let fill_rgb = if closed { fill.map(parse_hex_color).transpose()? } else { None };
    let opacity = opacity.clamp(0.0, 1.0);
    let width = stroke_width.max(0.0);
    let subtype: &[u8] = if closed { b"Polygon" } else { b"PolyLine" };

    let mut doc = Document::load_mem(bytes).map_err(cos_err)?;
    let page_no = u32::try_from(page)
        .ok()
        .map(|n| n + 1)
        .ok_or_else(|| CommandError::InvalidInput(format!("bad page index: {page}")))?;
    let page_id = *doc
        .get_pages()
        .get(&page_no)
        .ok_or_else(|| CommandError::InvalidInput(format!("page index out of range: {page}")))?;

    // `/BBox` covers every vertex + half the stroke width (the `/AP` clips to it).
    let pad = width.max(1.0);
    let bx0 = points.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min) - pad;
    let by0 = points.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min) - pad;
    let bx1 = points.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max) + pad;
    let by1 = points.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max) + pad;
    let rect_obj =
        Object::Array(vec![Object::Real(bx0), Object::Real(by0), Object::Real(bx1), Object::Real(by1)]);

    let vertices: Vec<Object> =
        points.iter().flat_map(|p| [Object::Real(p[0]), Object::Real(p[1])]).collect();

    let content = polygon_appearance_content(closed, points, (sr, sg, sb), fill_rgb, width);

    let mut gs = Dictionary::new();
    gs.set("ca", Object::Real(opacity));
    gs.set("CA", Object::Real(opacity));
    let mut ext = Dictionary::new();
    ext.set("GS", Object::Dictionary(gs));
    let mut resources = Dictionary::new();
    resources.set("ExtGState", Object::Dictionary(ext));

    let mut ap_dict = Dictionary::new();
    ap_dict.set("Type", Object::Name(b"XObject".to_vec()));
    ap_dict.set("Subtype", Object::Name(b"Form".to_vec()));
    ap_dict.set("FormType", Object::Integer(1));
    ap_dict.set("BBox", rect_obj.clone());
    ap_dict.set("Resources", Object::Dictionary(resources));
    let ap_id = doc.add_object(Stream::new(ap_dict, content.into_bytes()));
    let mut ap = Dictionary::new();
    ap.set("N", Object::Reference(ap_id));

    let mut bs = Dictionary::new();
    bs.set("W", Object::Real(width));
    bs.set("S", Object::Name(b"S".to_vec()));

    let mut annot = Dictionary::new();
    annot.set("Type", Object::Name(b"Annot".to_vec()));
    annot.set("Subtype", Object::Name(subtype.to_vec()));
    annot.set("NM", Object::string_literal(uuid::Uuid::new_v4().to_string()));
    annot.set("Vertices", Object::Array(vertices));
    annot.set("Rect", rect_obj);
    annot.set("C", Object::Array(vec![Object::Real(sr), Object::Real(sg), Object::Real(sb)]));
    if let Some((fr, fg, fb)) = fill_rgb {
        annot.set("IC", Object::Array(vec![Object::Real(fr), Object::Real(fg), Object::Real(fb)]));
    }
    annot.set("CA", Object::Real(opacity));
    annot.set("BS", Object::Dictionary(bs));
    annot.set("F", Object::Integer(4)); // Print
    annot.set("P", Object::Reference(page_id));
    annot.set("AP", Object::Dictionary(ap));
    let annot_id = doc.add_object(Object::Dictionary(annot));

    append_annotation(&mut doc, page_id, annot_id)?;
    let mut buf = Vec::new();
    doc.save_to(&mut buf)?;
    Ok(buf)
}

/// The `/AP` content stream for a polygon/polyline: `m` to the first vertex,
/// `l` to the rest; `h` closes a polygon; paint fill+stroke / fill / stroke.
fn polygon_appearance_content(
    closed: bool,
    points: &[[f32; 2]],
    (sr, sg, sb): (f32, f32, f32),
    fill: Option<(f32, f32, f32)>,
    width: f32,
) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(out, "/GS gs");
    let _ = writeln!(out, "{width:.2} w");
    let _ = writeln!(out, "{sr:.4} {sg:.4} {sb:.4} RG");
    if let Some((fr, fg, fb)) = fill {
        let _ = writeln!(out, "{fr:.4} {fg:.4} {fb:.4} rg");
    }
    if let Some(&[fx, fy]) = points.first() {
        let _ = writeln!(out, "{fx:.2} {fy:.2} m");
    }
    for p in &points[1..] {
        let _ = writeln!(out, "{:.2} {:.2} l", p[0], p[1]);
    }
    if closed {
        let _ = writeln!(out, "h");
    }
    let paint = match (closed && fill.is_some(), width > 0.0) {
        (true, true) => "B",
        (true, false) => "f",
        (false, _) => "S",
    };
    let _ = writeln!(out, "{paint}");
    out
}

/// SPEC: P3-ANN-005 — add a freehand `/Ink` annotation through `points`
/// (`[x, y, pressure]` in page coords; smoothing already applied on the
/// frontend), with a generated `/AP`. Pressure modulates the stroke width: the
/// `/AP` is a *filled ribbon* — the centreline offset by ±`base_width·f(pressure)/2`
/// along each local normal — so it renders identically in every viewer (it is
/// just a fill) and a uniform pressure (mouse/trackpad report a constant `0.5`)
/// degrades to a constant-width stroke. One stroke == one annotation for now.
pub fn add_ink(
    bytes: &[u8],
    page: usize,
    points: &[[f32; 3]],
    color: &str,
    opacity: f32,
    base_width: f32,
) -> Result<Vec<u8>, CommandError> {
    // Drop points coincident with their predecessor — a zero-length segment has
    // no normal and would blow up the ribbon offset.
    let pts: Vec<[f32; 3]> = dedupe_ink_points(points);
    if pts.len() < 2 {
        return Err(CommandError::InvalidInput("ink needs at least 2 distinct points".into()));
    }
    let (cr, cg, cb) = parse_hex_color(color)?;
    let opacity = opacity.clamp(0.0, 1.0);
    let width = base_width.max(0.1);

    let mut doc = Document::load_mem(bytes).map_err(cos_err)?;
    let page_no = u32::try_from(page)
        .ok()
        .map(|n| n + 1)
        .ok_or_else(|| CommandError::InvalidInput(format!("bad page index: {page}")))?;
    let page_id = *doc
        .get_pages()
        .get(&page_no)
        .ok_or_else(|| CommandError::InvalidInput(format!("page index out of range: {page}")))?;

    // `/BBox` covers every vertex + the widest half-stroke the ribbon reaches
    // (the `/AP` form clips to it, so an under-pad would shave a hard press).
    let pad = pts.iter().map(|p| ink_half_width(p[2], width)).fold(1.0_f32, f32::max);
    let bx0 = pts.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min) - pad;
    let by0 = pts.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min) - pad;
    let bx1 = pts.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max) + pad;
    let by1 = pts.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max) + pad;
    let rect_obj =
        Object::Array(vec![Object::Real(bx0), Object::Real(by0), Object::Real(bx1), Object::Real(by1)]);

    // `/InkList` is an array of sub-paths; one stroke == one flat `[x y x y …]`.
    let ink_path: Vec<Object> =
        pts.iter().flat_map(|p| [Object::Real(p[0]), Object::Real(p[1])]).collect();

    let content = ink_appearance_content(&pts, (cr, cg, cb), width);

    let mut gs = Dictionary::new();
    gs.set("ca", Object::Real(opacity));
    gs.set("CA", Object::Real(opacity));
    let mut ext = Dictionary::new();
    ext.set("GS", Object::Dictionary(gs));
    let mut resources = Dictionary::new();
    resources.set("ExtGState", Object::Dictionary(ext));

    let mut ap_dict = Dictionary::new();
    ap_dict.set("Type", Object::Name(b"XObject".to_vec()));
    ap_dict.set("Subtype", Object::Name(b"Form".to_vec()));
    ap_dict.set("FormType", Object::Integer(1));
    ap_dict.set("BBox", rect_obj.clone());
    ap_dict.set("Resources", Object::Dictionary(resources));
    let ap_id = doc.add_object(Stream::new(ap_dict, content.into_bytes()));
    let mut ap = Dictionary::new();
    ap.set("N", Object::Reference(ap_id));

    let mut bs = Dictionary::new();
    bs.set("W", Object::Real(width));
    bs.set("S", Object::Name(b"S".to_vec()));

    let mut annot = Dictionary::new();
    annot.set("Type", Object::Name(b"Annot".to_vec()));
    annot.set("Subtype", Object::Name(b"Ink".to_vec()));
    annot.set("NM", Object::string_literal(uuid::Uuid::new_v4().to_string()));
    annot.set("InkList", Object::Array(vec![Object::Array(ink_path)]));
    annot.set("Rect", rect_obj);
    annot.set("C", Object::Array(vec![Object::Real(cr), Object::Real(cg), Object::Real(cb)]));
    annot.set("CA", Object::Real(opacity));
    annot.set("BS", Object::Dictionary(bs));
    annot.set("F", Object::Integer(4)); // Print
    annot.set("P", Object::Reference(page_id));
    annot.set("AP", Object::Dictionary(ap));
    let annot_id = doc.add_object(Object::Dictionary(annot));

    append_annotation(&mut doc, page_id, annot_id)?;
    let mut buf = Vec::new();
    doc.save_to(&mut buf)?;
    Ok(buf)
}

/// Drop points within `< 0.01pt` of their predecessor (x/y only). Pressure is
/// carried through from the surviving point.
fn dedupe_ink_points(points: &[[f32; 3]]) -> Vec<[f32; 3]> {
    let mut out: Vec<[f32; 3]> = Vec::with_capacity(points.len());
    for &p in points {
        match out.last() {
            Some(&last) if (p[0] - last[0]).hypot(p[1] - last[1]) < 0.01 => {}
            _ => out.push(p),
        }
    }
    out
}

/// The half-width contributed by a sample's pressure. A reported pressure of
/// `0.5` (the neutral value mice/trackpads emit) maps to the base half-width;
/// the range `[0,1]` fans out to `[0.4, 1.3]×` so a hard press is visibly fatter
/// without a feather-light touch vanishing.
fn ink_half_width(pressure: f32, base_width: f32) -> f32 {
    let p = if pressure <= 0.0 { 0.5 } else { pressure.clamp(0.0, 1.0) };
    base_width * 0.5 * (0.4 + 1.8 * p)
}

/// The unit normal `(-uy, ux)` of segment `a → b`, or `None` if degenerate.
fn segment_normal(a: [f32; 3], b: [f32; 3]) -> Option<(f32, f32)> {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = dx.hypot(dy);
    if len < f32::EPSILON {
        return None;
    }
    Some((-dy / len, dx / len))
}

/// The `/AP` content stream for an ink stroke: a filled ribbon around the
/// smoothed centreline. For each sample we average the normals of its adjacent
/// segments and offset ±`ink_half_width(pressure)`, producing a left edge and a
/// right edge; the filled outline is `left… + right(reversed)`. Nonzero-winding
/// (`f`) so a self-crossing loop fills solid rather than punching a hole.
fn ink_appearance_content(points: &[[f32; 3]], (cr, cg, cb): (f32, f32, f32), base_width: f32) -> String {
    use std::fmt::Write as _;

    // Per-sample normal = average of the adjacent segment normals (carry the last
    // valid one across degenerate segments so a backtrack doesn't drop a point).
    let n = points.len();
    let mut normals: Vec<(f32, f32)> = Vec::with_capacity(n);
    let mut prev_seg: Option<(f32, f32)> = None;
    for i in 0..n {
        let before = if i > 0 { segment_normal(points[i - 1], points[i]) } else { None };
        let after = if i + 1 < n { segment_normal(points[i], points[i + 1]) } else { None };
        let here = match (before.or(prev_seg), after) {
            (Some(a), Some(b)) => {
                let (sx, sy) = (a.0 + b.0, a.1 + b.1);
                let len = sx.hypot(sy);
                if len < f32::EPSILON { b } else { (sx / len, sy / len) }
            }
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => (0.0, 1.0),
        };
        if let Some(seg) = after.or(before) {
            prev_seg = Some(seg);
        }
        normals.push(here);
    }

    let mut left: Vec<(f32, f32)> = Vec::with_capacity(n);
    let mut right: Vec<(f32, f32)> = Vec::with_capacity(n);
    for (i, &p) in points.iter().enumerate() {
        let hw = ink_half_width(p[2], base_width);
        let (nx, ny) = normals[i];
        left.push((p[0] + nx * hw, p[1] + ny * hw));
        right.push((p[0] - nx * hw, p[1] - ny * hw));
    }

    let mut out = String::new();
    let _ = writeln!(out, "/GS gs");
    let _ = writeln!(out, "{cr:.4} {cg:.4} {cb:.4} rg");
    let (fx, fy) = left[0];
    let _ = writeln!(out, "{fx:.2} {fy:.2} m");
    for &(x, y) in &left[1..] {
        let _ = writeln!(out, "{x:.2} {y:.2} l");
    }
    for &(x, y) in right.iter().rev() {
        let _ = writeln!(out, "{x:.2} {y:.2} l");
    }
    let _ = writeln!(out, "h");
    let _ = writeln!(out, "f");
    out
}

/// The `/AP` content stream for a shape, drawn in absolute page coords (the
/// form's `BBox` == `Rect`). The path is inset by half the stroke width so the
/// stroke stays inside `/Rect`. An ellipse is the standard 4-Bézier (kappa)
/// approximation. Fill uses `/GS` opacity via the resource set on the form.
fn shape_appearance_content(
    kind: &str,
    rect: [f32; 4],
    (sr, sg, sb): (f32, f32, f32),
    fill: Option<(f32, f32, f32)>,
    width: f32,
) -> String {
    use std::fmt::Write as _;
    let [x0, y0, x1, y1] = rect;
    let hw = width / 2.0;
    let (ix0, iy0, ix1, iy1) = (x0 + hw, y0 + hw, x1 - hw, y1 - hw);

    let mut out = String::new();
    let _ = writeln!(out, "/GS gs");
    let _ = writeln!(out, "{width:.2} w");
    let _ = writeln!(out, "{sr:.4} {sg:.4} {sb:.4} RG");
    if let Some((fr, fg, fb)) = fill {
        let _ = writeln!(out, "{fr:.4} {fg:.4} {fb:.4} rg");
    }
    // Paint operator: fill+stroke (B), fill only (f), or stroke only (S).
    let paint = match (fill.is_some(), width > 0.0) {
        (true, true) => "B",
        (true, false) => "f",
        (false, _) => "S",
    };

    if kind == "rectangle" {
        let _ = writeln!(out, "{ix0:.2} {iy0:.2} {:.2} {:.2} re", ix1 - ix0, iy1 - iy0);
        let _ = writeln!(out, "{paint}");
    } else {
        // Ellipse as four cubic Béziers (kappa ≈ 0.5523), starting at the right
        // anchor and going counter-clockwise: right→top→left→bottom→right.
        let cx = (ix0 + ix1) / 2.0;
        let cy = (iy0 + iy1) / 2.0;
        let rx = (ix1 - ix0) / 2.0;
        let ry = (iy1 - iy0) / 2.0;
        let kx = rx * 0.552_284_8;
        let ky = ry * 0.552_284_8;
        let _ = writeln!(out, "{:.2} {:.2} m", cx + rx, cy);
        let _ = writeln!(
            out,
            "{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c",
            cx + rx, cy + ky, cx + kx, cy + ry, cx, cy + ry
        );
        let _ = writeln!(
            out,
            "{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c",
            cx - kx, cy + ry, cx - rx, cy + ky, cx - rx, cy
        );
        let _ = writeln!(
            out,
            "{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c",
            cx - rx, cy - ky, cx - kx, cy - ry, cx, cy - ry
        );
        let _ = writeln!(
            out,
            "{:.2} {:.2} {:.2} {:.2} {:.2} {:.2} c",
            cx + kx, cy - ry, cx + rx, cy - ky, cx + rx, cy
        );
        let _ = writeln!(out, "h");
        let _ = writeln!(out, "{paint}");
    }
    out
}

/// Parse `#rrggbb` into RGB components in 0..=1.
#[allow(clippy::cast_precision_loss)] // 0..=255 → f32 is exact.
fn parse_hex_color(hex: &str) -> Result<(f32, f32, f32), CommandError> {
    let h = hex.trim_start_matches('#');
    if h.len() != 6 || !h.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(CommandError::InvalidInput(format!("bad colour: {hex}")));
    }
    let comp = |s: &str| f32::from(u8::from_str_radix(s, 16).unwrap_or(0)) / 255.0;
    Ok((comp(&h[0..2]), comp(&h[2..4]), comp(&h[4..6])))
}

/// Bounding box [x0,y0,x1,y1] of all quads.
fn quads_bounds(quads: &[[f32; 8]]) -> (f32, f32, f32, f32) {
    let mut x0 = f32::INFINITY;
    let mut y0 = f32::INFINITY;
    let mut x1 = f32::NEG_INFINITY;
    let mut y1 = f32::NEG_INFINITY;
    for q in quads {
        let (a, b, c, d) = quad_bbox(q);
        x0 = x0.min(a);
        y0 = y0.min(b);
        x1 = x1.max(c);
        y1 = y1.max(d);
    }
    (x0, y0, x1, y1)
}

/// Bounding box of one quad's four corners.
fn quad_bbox(q: &[f32; 8]) -> (f32, f32, f32, f32) {
    let xs = [q[0], q[2], q[4], q[6]];
    let ys = [q[1], q[3], q[5], q[7]];
    (
        xs.iter().copied().fold(f32::INFINITY, f32::min),
        ys.iter().copied().fold(f32::INFINITY, f32::min),
        xs.iter().copied().fold(f32::NEG_INFINITY, f32::max),
        ys.iter().copied().fold(f32::NEG_INFINITY, f32::max),
    )
}

/// Append a small zigzag path (squiggly underline) to the content stream.
fn write_squiggle(out: &mut String, x0: f32, x1: f32, y: f32, amp: f32) {
    use std::fmt::Write as _;
    if (x1 - x0).abs() < 1.0 {
        return;
    }
    let _ = writeln!(out, "{x0:.2} {y:.2} m");
    let dir = if x1 >= x0 { 4.0_f32 } else { -4.0_f32 };
    let mut x = x0;
    let mut up = true;
    loop {
        x += dir;
        let past = if dir > 0.0 { x >= x1 } else { x <= x1 };
        let px = if past { x1 } else { x };
        let dy = if up { amp } else { -amp };
        let _ = writeln!(out, "{px:.2} {:.2} l", y + dy);
        up = !up;
        if past {
            break;
        }
    }
    let _ = writeln!(out, "S");
}

/// SPEC: P3-ANN-001 — remove every text-markup annotation (Highlight / Underline
/// / `StrikeOut` / Squiggly) from all pages, keeping any other annotations. GCs
/// the orphaned annotation dicts + their `/AP` streams via `prune_objects`.
pub fn clear_text_markup(bytes: &[u8]) -> Result<Vec<u8>, CommandError> {
    let mut doc = Document::load_mem(bytes).map_err(cos_err)?;
    let page_ids: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    let mut changed = false;

    for page_id in page_ids {
        let annots = doc
            .get_dictionary(page_id)
            .ok()
            .and_then(|p| p.get(b"Annots").ok().cloned());
        let (arr, indirect_id) = match annots {
            Some(Object::Array(a)) => (a, None),
            Some(Object::Reference(id)) => (
                doc.get_object(id).and_then(Object::as_array).cloned().unwrap_or_default(),
                Some(id),
            ),
            _ => continue,
        };
        if arr.is_empty() {
            continue;
        }

        let mut kept = Vec::with_capacity(arr.len());
        let mut removed_any = false;
        for obj in arr {
            let is_markup = obj
                .as_reference()
                .ok()
                .and_then(|id| doc.get_dictionary(id).ok())
                .and_then(|d| d.get(b"Subtype").and_then(Object::as_name).ok())
                .is_some_and(|n| {
                    n == b"Highlight" || n == b"Underline" || n == b"StrikeOut" || n == b"Squiggly"
                });
            if is_markup {
                removed_any = true;
            } else {
                kept.push(obj);
            }
        }
        if !removed_any {
            continue;
        }
        changed = true;

        match indirect_id {
            Some(id) => {
                if let Ok(obj) = doc.get_object_mut(id) {
                    *obj = Object::Array(kept);
                }
            }
            None => {
                doc.get_dictionary_mut(page_id)
                    .map_err(cos_err)?
                    .set("Annots", Object::Array(kept));
            }
        }
    }

    if changed {
        doc.prune_objects();
    }
    let mut buf = Vec::new();
    doc.save_to(&mut buf)?;
    Ok(buf)
}

/// Current time as a PDF date string `D:YYYYMMDDHHmmSSZ` (Hinnant's
/// civil-from-days, so no date-library dependency).
#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_sign_loss)]
fn pdf_date_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let day = (secs / 86_400) as i64;
    let tod = secs % 86_400;
    let (hh, mm, ss) = (tod / 3600, (tod % 3600) / 60, tod % 60);
    let z = day + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!("D:{year:04}{m:02}{d:02}{hh:02}{mm:02}{ss:02}Z")
}

/// Append `annot_id` to `page_id`'s `/Annots` (array, indirect array, or absent).
fn append_annotation(
    doc: &mut Document,
    page_id: ObjectId,
    annot_id: ObjectId,
) -> Result<(), CommandError> {
    let existing = doc
        .get_dictionary(page_id)
        .map_err(cos_err)?
        .get(b"Annots")
        .ok()
        .cloned();
    match existing {
        Some(Object::Reference(arr_id)) => {
            if let Ok(Object::Array(arr)) = doc.get_object_mut(arr_id) {
                arr.push(Object::Reference(annot_id));
            } else {
                return Err(CommandError::InvalidInput("malformed /Annots".into()));
            }
        }
        Some(Object::Array(mut arr)) => {
            arr.push(Object::Reference(annot_id));
            doc.get_dictionary_mut(page_id)
                .map_err(cos_err)?
                .set("Annots", Object::Array(arr));
        }
        _ => {
            doc.get_dictionary_mut(page_id)
                .map_err(cos_err)?
                .set("Annots", Object::Array(vec![Object::Reference(annot_id)]));
        }
    }
    Ok(())
}

/// The object id of the annotation whose `/NM` (name) equals `nm`, scanning all
/// pages. `/NM` is the stable handle the frontend uses to target update/delete.
fn find_annotation_by_nm(doc: &Document, nm: &str) -> Option<ObjectId> {
    for page_id in doc.get_pages().values() {
        let annots = doc.get_dictionary(*page_id).ok().and_then(|p| p.get(b"Annots").ok().cloned());
        let arr = match annots {
            Some(Object::Array(a)) => a,
            Some(Object::Reference(id)) => {
                doc.get_object(id).and_then(Object::as_array).cloned().unwrap_or_default()
            }
            _ => continue,
        };
        for obj in arr {
            let Ok(id) = obj.as_reference() else { continue };
            let matches = doc
                .get_dictionary(id)
                .ok()
                .and_then(|d| d.get(b"NM").and_then(Object::as_str).ok())
                .is_some_and(|s| s == nm.as_bytes());
            if matches {
                return Some(id);
            }
        }
    }
    None
}

/// SPEC: P3-ANN-002 — add a sticky note (`/Text` annotation) at `(x, y)` on
/// `page` (0-based) with `content`, `author` (`/T`), a timestamp, and `note_id`
/// as `/NM`. No `/AP` — readers draw their own note icon from `/Name`.
pub fn add_text_note(
    bytes: &[u8],
    note_id: &str,
    page: usize,
    x: f32,
    y: f32,
    content: &str,
    author: &str,
) -> Result<Vec<u8>, CommandError> {
    let mut doc = Document::load_mem(bytes).map_err(cos_err)?;
    let page_no = u32::try_from(page)
        .ok()
        .map(|n| n + 1)
        .ok_or_else(|| CommandError::InvalidInput(format!("bad page index: {page}")))?;
    let page_id = *doc
        .get_pages()
        .get(&page_no)
        .ok_or_else(|| CommandError::InvalidInput(format!("page index out of range: {page}")))?;

    let date = pdf_date_now();
    let mut annot = Dictionary::new();
    annot.set("Type", Object::Name(b"Annot".to_vec()));
    annot.set("Subtype", Object::Name(b"Text".to_vec()));
    annot.set(
        "Rect",
        Object::Array(vec![
            Object::Real(x),
            Object::Real(y),
            Object::Real(x + 18.0),
            Object::Real(y + 18.0),
        ]),
    );
    annot.set("Contents", Object::string_literal(content));
    annot.set("T", Object::string_literal(author));
    annot.set("NM", Object::string_literal(note_id));
    annot.set("M", Object::string_literal(date.clone()));
    annot.set("CreationDate", Object::string_literal(date));
    annot.set("Name", Object::Name(b"Note".to_vec()));
    annot.set("C", Object::Array(vec![Object::Real(1.0), Object::Real(0.82), Object::Real(0.0)]));
    annot.set("F", Object::Integer(28)); // Print | NoZoom | NoRotate
    annot.set("Open", Object::Boolean(false));
    annot.set("P", Object::Reference(page_id));
    let annot_id = doc.add_object(Object::Dictionary(annot));

    append_annotation(&mut doc, page_id, annot_id)?;
    let mut buf = Vec::new();
    doc.save_to(&mut buf)?;
    Ok(buf)
}

/// SPEC: P3-ANN-002 — update the note with `/NM == note_id`: new `/Contents` and
/// a fresh `/M` (modification date).
pub fn update_text_note(bytes: &[u8], note_id: &str, content: &str) -> Result<Vec<u8>, CommandError> {
    let mut doc = Document::load_mem(bytes).map_err(cos_err)?;
    let id = find_annotation_by_nm(&doc, note_id)
        .ok_or_else(|| CommandError::InvalidInput(format!("note not found: {note_id}")))?;
    let date = pdf_date_now();
    let dict = doc.get_dictionary_mut(id).map_err(cos_err)?;
    dict.set("Contents", Object::string_literal(content));
    dict.set("M", Object::string_literal(date));
    let mut buf = Vec::new();
    doc.save_to(&mut buf)?;
    Ok(buf)
}

/// Parse an `obj:<num> <gen>` handle's body (`"<num> <gen>"`) into an `ObjectId`.
fn parse_object_id(s: &str) -> Option<ObjectId> {
    let mut parts = s.split_whitespace();
    let num = parts.next()?.parse::<u32>().ok()?;
    let gen = parts.next()?.parse::<u16>().ok()?;
    Some((num, gen))
}

/// SPEC: P3-ANN-002 / P3-ANN-012 — delete the annotation identified by `handle`
/// from its page's `/Annots`; GCs it (+ any owned objects) via `prune_objects`.
/// No-op if absent. `handle` is either a `/NM` (the stable name our writers
/// stamp on every annotation) or, for an annotation that lacks one, the
/// `obj:<num> <gen>` object id `read_annotations` synthesized.
pub fn delete_annotation(bytes: &[u8], handle: &str) -> Result<Vec<u8>, CommandError> {
    let mut doc = Document::load_mem(bytes).map_err(cos_err)?;
    let target = match handle.strip_prefix("obj:") {
        Some(obj) => parse_object_id(obj),
        None => find_annotation_by_nm(&doc, handle),
    };
    let Some(target) = target else {
        return Ok(bytes.to_vec());
    };

    let page_ids: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    for page_id in page_ids {
        let existing = doc.get_dictionary(page_id).ok().and_then(|p| p.get(b"Annots").ok().cloned());
        let (arr, indirect_id) = match existing {
            Some(Object::Array(a)) => (a, None),
            Some(Object::Reference(id)) => (
                doc.get_object(id).and_then(Object::as_array).cloned().unwrap_or_default(),
                Some(id),
            ),
            _ => continue,
        };
        if !arr.iter().any(|o| o.as_reference().ok() == Some(target)) {
            continue;
        }
        let kept: Vec<Object> = arr
            .into_iter()
            .filter(|o| o.as_reference().ok() != Some(target))
            .collect();
        match indirect_id {
            Some(id) => {
                if let Ok(obj) = doc.get_object_mut(id) {
                    *obj = Object::Array(kept);
                }
            }
            None => {
                doc.get_dictionary_mut(page_id).map_err(cos_err)?.set("Annots", Object::Array(kept));
            }
        }
    }

    doc.prune_objects();
    let mut buf = Vec::new();
    doc.save_to(&mut buf)?;
    Ok(buf)
}

/// A sticky note read back out of a PDF (P3.B2b). The inverse of the fields
/// [`add_text_note`] writes: `nm` is `/NM` (the stable handle), `page` is
/// 0-based, `(x, y)` is the `/Rect` lower-left, and `content` / `author` are
/// `/Contents` / `/T`. Serialized to the frontend, which projects it into the
/// note overlay store.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteData {
    pub nm: String,
    pub page: usize,
    pub x: f32,
    pub y: f32,
    pub content: String,
    pub author: String,
}

/// SPEC: P3-ANN-002 (re-openable clause) — read every `/Text` annotation
/// (sticky note) out of the document, in page order. The inverse of
/// [`add_text_note`]; lets the frontend rebuild its note overlay from the PDF on
/// open (and re-sync after undo/redo) so saved notes are re-openable in-app.
///
/// A note authored elsewhere may lack `/NM`; we synthesize a stable fallback id
/// from its object id so it still renders (and a later edit writes a real `/NM`).
/// Only ASCII `/Contents` / `/T` are decoded faithfully — UTF-16BE /
/// `PDFDocEncoding` is out of scope (we only guarantee round-trip of notes we
/// wrote).
pub fn read_text_notes(bytes: &[u8]) -> Result<Vec<NoteData>, CommandError> {
    let doc = Document::load_mem(bytes).map_err(cos_err)?;
    let mut notes = Vec::new();

    // `get_pages` is a BTreeMap keyed by 1-based page number, so iteration is
    // already in page order; the 0-based index is `page_no - 1`.
    for (page_no, page_id) in doc.get_pages() {
        let page = (page_no - 1) as usize;
        let annots = doc.get_dictionary(page_id).ok().and_then(|p| p.get(b"Annots").ok().cloned());
        let arr = match annots {
            Some(Object::Array(a)) => a,
            Some(Object::Reference(id)) => {
                doc.get_object(id).and_then(Object::as_array).cloned().unwrap_or_default()
            }
            _ => continue,
        };
        for obj in arr {
            let Ok(id) = obj.as_reference() else { continue };
            let Ok(dict) = doc.get_dictionary(id) else { continue };
            if dict.get(b"Subtype").and_then(Object::as_name).ok() != Some(&b"Text"[..]) {
                continue;
            }
            let (x, y) = rect_lower_left(dict);
            let nm = dict.get(b"NM").and_then(Object::as_str).ok().map_or_else(
                || format!("obj-{}-{}", id.0, id.1),
                |s| String::from_utf8_lossy(s).into_owned(),
            );
            notes.push(NoteData {
                nm,
                page,
                x,
                y,
                content: str_field(dict, b"Contents"),
                author: str_field(dict, b"T"),
            });
        }
    }
    Ok(notes)
}

/// A PDF string field as a lossy UTF-8 `String`, or `""` when absent.
fn str_field(dict: &Dictionary, key: &[u8]) -> String {
    dict.get(key)
        .and_then(Object::as_str)
        .ok()
        .map_or_else(String::new, |s| String::from_utf8_lossy(s).into_owned())
}

/// The `(x, y)` lower-left of an annotation's `/Rect`, or `(0, 0)` if malformed.
/// Page coordinates are small (bounded by `/MediaBox`), so an `i64`-typed Rect
/// integer fits an `f32` without meaningful loss.
#[allow(clippy::cast_precision_loss)]
fn rect_lower_left(dict: &Dictionary) -> (f32, f32) {
    let Ok(rect) = dict.get(b"Rect").and_then(Object::as_array) else {
        return (0.0, 0.0);
    };
    let num = |i: usize| match rect.get(i) {
        Some(Object::Real(r)) => *r,
        Some(Object::Integer(n)) => *n as f32,
        _ => 0.0,
    };
    (num(0), num(1))
}

/// The four `/Rect` components `[x0, y0, x1, y1]`, or zeros if malformed.
#[allow(clippy::cast_precision_loss)]
fn rect_bounds(dict: &Dictionary) -> [f32; 4] {
    let Ok(rect) = dict.get(b"Rect").and_then(Object::as_array) else {
        return [0.0; 4];
    };
    let num = |i: usize| match rect.get(i) {
        Some(Object::Real(r)) => *r,
        Some(Object::Integer(n)) => *n as f32,
        _ => 0.0,
    };
    [num(0), num(1), num(2), num(3)]
}

/// One annotation as the sidebar (P3.D1) sees it: a stable-within-this-load `id`
/// (the lopdf object id), its 0-based `page`, a `kind` tag, `/Rect` bounds (for
/// the selection highlight), `/Contents`, `/T` (author), and `/M` parsed to epoch
/// millis. Serialized to the frontend annotation panel.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationInfo {
    pub id: String,
    pub page: usize,
    pub kind: String,
    pub rect: [f32; 4],
    pub contents: String,
    pub author: String,
    pub modified: Option<i64>,
}

/// Map an annotation `/Subtype` to the sidebar `kind`, or `None` for subtypes we
/// don't surface (links, form widgets, popups, …).
fn annotation_kind(subtype: &[u8]) -> Option<&'static str> {
    match subtype {
        b"Highlight" => Some("highlight"),
        b"Underline" => Some("underline"),
        b"StrikeOut" => Some("strikeout"),
        b"Squiggly" => Some("squiggly"),
        b"Text" => Some("note"),
        b"FreeText" => Some("freetext"),
        b"Square" => Some("rectangle"),
        b"Circle" => Some("ellipse"),
        b"Line" => Some("line"),
        b"Polygon" => Some("polygon"),
        b"PolyLine" => Some("polyline"),
        b"Ink" => Some("ink"),
        b"Stamp" => Some("stamp"),
        _ => None,
    }
}

/// SPEC: P3-ANN-008 — read every (supported) annotation out of the document, in
/// page order, for the annotation sidebar. Read-only; the inverse of nothing in
/// particular — it surfaces whatever the write paths put on the page. Foreign
/// subtypes (`/Link`, `/Widget`, `/Popup`, …) are skipped.
pub fn read_annotations(bytes: &[u8]) -> Result<Vec<AnnotationInfo>, CommandError> {
    let doc = Document::load_mem(bytes).map_err(cos_err)?;
    let mut out = Vec::new();

    for (page_no, page_id) in doc.get_pages() {
        let page = (page_no - 1) as usize;
        let annots = doc.get_dictionary(page_id).ok().and_then(|p| p.get(b"Annots").ok().cloned());
        let arr = match annots {
            Some(Object::Array(a)) => a,
            Some(Object::Reference(id)) => {
                doc.get_object(id).and_then(Object::as_array).cloned().unwrap_or_default()
            }
            _ => continue,
        };
        for obj in arr {
            let Ok(id) = obj.as_reference() else { continue };
            let Ok(dict) = doc.get_dictionary(id) else { continue };
            let Some(kind) = dict.get(b"Subtype").and_then(Object::as_name).ok().and_then(annotation_kind)
            else {
                continue;
            };
            // Prefer the stable `/NM` (every annotation we write has one) as the
            // delete handle; fall back to a `obj:<num> <gen>` object id for
            // annotations authored elsewhere that lack a name.
            let handle = dict.get(b"NM").and_then(Object::as_str).ok().map_or_else(
                || format!("obj:{} {}", id.0, id.1),
                |nm| String::from_utf8_lossy(nm).into_owned(),
            );
            out.push(AnnotationInfo {
                id: handle,
                page,
                kind: kind.to_owned(),
                rect: rect_bounds(dict),
                contents: str_field(dict, b"Contents"),
                author: str_field(dict, b"T"),
                modified: dict.get(b"M").and_then(Object::as_str).ok().and_then(parse_pdf_date),
            });
        }
    }
    Ok(out)
}

/// Parse a PDF date string (`D:YYYYMMDDHHmmSS…`, the form [`pdf_date_now`] writes)
/// into epoch milliseconds. Time-of-day is optional; anything malformed → `None`.
/// The inverse of `pdf_date_now`'s civil-from-days, using Hinnant's
/// `days_from_civil`.
#[allow(clippy::cast_possible_wrap)]
fn parse_pdf_date(raw: &[u8]) -> Option<i64> {
    let s = std::str::from_utf8(raw).ok()?;
    let s = s.strip_prefix("D:").unwrap_or(s);
    let digits: &str = s.get(..14).or_else(|| s.get(..8))?;
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let part = |a: usize, b: usize| digits.get(a..b).and_then(|t| t.parse::<i64>().ok());
    let year = part(0, 4)?;
    let month = part(4, 6)?;
    let day = part(6, 8)?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let (hh, mm, ss) = if digits.len() >= 14 {
        (part(8, 10)?, part(10, 12)?, part(12, 14)?)
    } else {
        (0, 0, 0)
    };

    // Hinnant days_from_civil.
    let y = if month <= 2 { year - 1 } else { year };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;

    Some((days * 86_400 + hh * 3600 + mm * 60 + ss) * 1000)
}
