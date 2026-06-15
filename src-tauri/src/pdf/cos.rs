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

/// SPEC: P3-ANN-002 — delete the annotation with `/NM == note_id` from its page's
/// `/Annots`; GCs it (+ any owned objects) via `prune_objects`. No-op if absent.
pub fn delete_annotation(bytes: &[u8], note_id: &str) -> Result<Vec<u8>, CommandError> {
    let mut doc = Document::load_mem(bytes).map_err(cos_err)?;
    let Some(target) = find_annotation_by_nm(&doc, note_id) else {
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
