//! Flatten annotations into the page content streams (P3.E2).
//!
//! SPEC: P3-ANN-011 — render every annotation permanently into the page content,
//! so the markup becomes part of the page (no longer a separate, editable
//! annotation). A COS (lopdf) byte→byte transform, like [`crate::pdf::cos`]'s
//! other structural edits — never a shared live `PDFium` handle (docs/04). The
//! caller wraps it in a `cos_edit` whose inverse is a pre-flatten byte snapshot,
//! so it's undoable *in-session* but gone once the file is saved + reopened —
//! exactly the spec's "not undoable from a saved file."
//!
//! Mechanism (the same content-append the resize edit uses): for each annotation
//! that carries a renderable `/AP /N` appearance form, register that *existing*
//! form under the page's `/Resources /XObject` and append a self-contained
//! `q <matrix> cm /<name> Do Q` fragment to a new content stream spliced onto the
//! end of `/Contents`; then drop the annotation from `/Annots`. The placement
//! matrix maps the form's `/BBox` (through its `/Matrix`) onto the annotation's
//! `/Rect` (PDF 32000-1 §12.5.5) — the identity for our own annotations, where
//! `BBox == Rect` and `/Matrix` is absent.
//!
//! `/AP`-less annotations (our sticky notes + replies, plus `/Link`/`/Popup`)
//! have no appearance to bake, so they're **kept** as live annotations.

use std::fmt::Write as _;

use lopdf::{Dictionary, Document, Object, ObjectId, Stream};

use crate::error::CommandError;

#[allow(clippy::needless_pass_by_value)]
fn flat_err(e: lopdf::Error) -> CommandError {
    CommandError::PdfError(format!("flatten: {e}"))
}

/// SPEC: P3-ANN-011 — flatten every `/AP`-bearing annotation in `bytes` into the
/// page content streams, returning the new document. Annotations without a
/// renderable appearance are left untouched.
pub fn flatten_annotations(bytes: &[u8]) -> Result<Vec<u8>, CommandError> {
    let mut doc = Document::load_mem(bytes).map_err(flat_err)?;
    let page_ids: Vec<ObjectId> = doc.get_pages().values().copied().collect();
    let mut changed = false;

    for page_id in page_ids {
        if flatten_page(&mut doc, page_id)? {
            changed = true;
        }
    }

    if changed {
        // GC the now-orphaned annotation dicts. The `/AP` form streams survive —
        // the page `/Resources /XObject` references them after `add_xobject`.
        doc.prune_objects();
    }
    let mut buf = Vec::new();
    doc.save_to(&mut buf)?;
    Ok(buf)
}

/// Flatten one page's `/AP`-bearing annotations. Returns whether anything changed.
fn flatten_page(doc: &mut Document, page_id: ObjectId) -> Result<bool, CommandError> {
    let annots = match doc.get_dictionary(page_id).ok().and_then(|p| p.get(b"Annots").ok().cloned()) {
        Some(Object::Array(a)) => a,
        Some(Object::Reference(id)) => {
            doc.get_object(id).and_then(Object::as_array).cloned().unwrap_or_default()
        }
        _ => return Ok(false),
    };
    if annots.is_empty() {
        return Ok(false);
    }

    // Read pass (immutable): split into placements (flatten) + kept (leave).
    let mut placements: Vec<(ObjectId, [f32; 6])> = Vec::new();
    let mut kept: Vec<Object> = Vec::new();
    for obj in annots {
        let placement = obj
            .as_reference()
            .ok()
            .and_then(|id| doc.get_dictionary(id).ok())
            .and_then(|annot| flatten_placement(doc, annot));
        match placement {
            Some(p) => placements.push(p),
            None => kept.push(obj), // /AP-less or unresolvable — keep it live
        }
    }
    if placements.is_empty() {
        return Ok(false);
    }

    // Mutate pass: register each form + build one content fragment.
    ensure_own_resources(doc, page_id)?;
    let mut frag = String::new();
    for (form_id, m) in &placements {
        let name = format!("VPFlat{}", form_id.0);
        doc.add_xobject(page_id, name.clone().into_bytes(), *form_id).map_err(flat_err)?;
        // q <a b c d e f> cm /Name Do Q — self-contained, so the form's own
        // graphics state can't leak into later fragments or page content.
        let _ = writeln!(
            frag,
            "q {:.4} {:.4} {:.4} {:.4} {:.4} {:.4} cm /{} Do Q",
            m[0], m[1], m[2], m[3], m[4], m[5], name
        );
    }
    let frag_id = doc.add_object(Stream::new(Dictionary::new(), frag.into_bytes()));

    // Splice the fragment onto the end of /Contents (so it paints on top, the way
    // annotations render over the page). Don't decode existing content — just
    // append a reference, the resize edit's lesson.
    let existing: Vec<Object> = match doc.get_dictionary(page_id).map_err(flat_err)?.get(b"Contents") {
        Ok(Object::Reference(id)) => vec![Object::Reference(*id)],
        Ok(Object::Array(arr)) => arr.clone(),
        _ => Vec::new(),
    };
    let mut contents = existing;
    contents.push(Object::Reference(frag_id));

    let pd = doc.get_dictionary_mut(page_id).map_err(flat_err)?;
    pd.set("Contents", Object::Array(contents));
    if kept.is_empty() {
        pd.remove(b"Annots");
    } else {
        pd.set("Annots", Object::Array(kept));
    }
    Ok(true)
}

/// If an annotation has a renderable `/AP /N` form stream, return that form's
/// object id + the matrix that places it onto the annotation's `/Rect`. `None`
/// for `/AP`-less or unresolvable annotations (left live by the caller).
fn flatten_placement(doc: &Document, annot: &Dictionary) -> Option<(ObjectId, [f32; 6])> {
    let rect = normalized_rect(&read_nums(annot, b"Rect")?)?;
    let form_id = appearance_form_id(doc, annot)?;
    let form = doc.get_object(form_id).and_then(Object::as_stream).ok()?;
    let bbox = read_nums(&form.dict, b"BBox").and_then(|v| four(&v))?;
    let matrix = read_nums(&form.dict, b"Matrix")
        .and_then(|v| six(&v))
        .unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    let m = appearance_matrix(bbox, matrix, rect)?;
    Some((form_id, m))
}

/// Resolve an annotation's `/AP /N` to the object id of its appearance form
/// stream. Handles a direct stream reference and an appearance sub-dictionary
/// (states keyed by `/AS`). `None` when there's no usable stream.
fn appearance_form_id(doc: &Document, annot: &Dictionary) -> Option<ObjectId> {
    let ap = annot.get(b"AP").and_then(Object::as_dict).ok()?;
    let n = ap.get(b"N").ok()?;
    match n {
        // Common path: /N is a reference to the appearance stream.
        Object::Reference(id) => match doc.get_object(*id).ok()? {
            Object::Stream(_) => Some(*id),
            // /N is itself an appearance sub-dictionary (states) — pick /AS.
            Object::Dictionary(states) => state_stream_id(doc, annot, states),
            _ => None,
        },
        // Inline appearance sub-dictionary.
        Object::Dictionary(states) => state_stream_id(doc, annot, states),
        _ => None,
    }
}

/// The appearance-state stream id selected by the annotation's `/AS` name.
fn state_stream_id(doc: &Document, annot: &Dictionary, states: &Dictionary) -> Option<ObjectId> {
    let as_name = annot.get(b"AS").and_then(Object::as_name).ok()?;
    let id = states.get(as_name).ok()?.as_reference().ok()?;
    matches!(doc.get_object(id).ok()?, Object::Stream(_)).then_some(id)
}

/// PDF 32000-1 §12.5.5: map the form's `/BBox` (transformed by its `/Matrix`)
/// onto the annotation's `/Rect`. Returns the `cm` matrix `[a b c d e f]`, or
/// `None` for a degenerate (zero-area) transformed box.
#[allow(clippy::many_single_char_names)]
fn appearance_matrix(bbox: [f32; 4], matrix: [f32; 6], rect: [f32; 4]) -> Option<[f32; 6]> {
    let [a, b, c, d, e, f] = matrix;
    let tx = |x: f32, y: f32| a * x + c * y + e;
    let ty = |x: f32, y: f32| b * x + d * y + f;
    // The four BBox corners through the form matrix → their bounding box.
    let corners = [(bbox[0], bbox[1]), (bbox[2], bbox[1]), (bbox[2], bbox[3]), (bbox[0], bbox[3])];
    let xs = corners.map(|(x, y)| tx(x, y));
    let ys = corners.map(|(x, y)| ty(x, y));
    let tx0 = xs.iter().copied().fold(f32::INFINITY, f32::min);
    let tx1 = xs.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let ty0 = ys.iter().copied().fold(f32::INFINITY, f32::min);
    let ty1 = ys.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let (bw, bh) = (tx1 - tx0, ty1 - ty0);
    if bw.abs() < f32::EPSILON || bh.abs() < f32::EPSILON {
        return None;
    }
    let [rx0, ry0, rx1, ry1] = rect;
    let sx = (rx1 - rx0) / bw;
    let sy = (ry1 - ry0) / bh;
    Some([sx, 0.0, 0.0, sy, rx0 - sx * tx0, ry0 - sy * ty0])
}

/// Ensure the page has its **own** `/Resources` before `add_xobject` adds to it —
/// `get_or_create_resources` would otherwise create an empty one, shadowing any
/// resources inherited from `/Pages` and breaking the existing content. Clones
/// the effective (inherited) resource dict down when the page has none of its own.
fn ensure_own_resources(doc: &mut Document, page_id: ObjectId) -> Result<(), CommandError> {
    if doc.get_dictionary(page_id).map_err(flat_err)?.has(b"Resources") {
        return Ok(()); // already its own (direct dict or a reference)
    }
    let inherited = {
        let (_own, ids) = doc.get_page_resources(page_id).map_err(flat_err)?;
        ids.first().copied()
    }
    .and_then(|id| doc.get_object(id).ok())
    .and_then(|o| o.as_dict().ok())
    .cloned()
    .unwrap_or_default();
    doc.get_dictionary_mut(page_id).map_err(flat_err)?.set("Resources", Object::Dictionary(inherited));
    Ok(())
}

// --- small numeric readers ---

#[allow(clippy::cast_precision_loss)]
fn obj_f32(o: &Object) -> Option<f32> {
    match o {
        Object::Real(r) => Some(*r),
        Object::Integer(n) => Some(*n as f32),
        _ => None,
    }
}

fn read_nums(dict: &Dictionary, key: &[u8]) -> Option<Vec<f32>> {
    Some(dict.get(key).and_then(Object::as_array).ok()?.iter().filter_map(obj_f32).collect())
}

fn four(v: &[f32]) -> Option<[f32; 4]> {
    (v.len() >= 4).then(|| [v[0], v[1], v[2], v[3]])
}

fn six(v: &[f32]) -> Option<[f32; 6]> {
    (v.len() >= 6).then(|| [v[0], v[1], v[2], v[3], v[4], v[5]])
}

/// A `/Rect` as `[x0,y0,x1,y1]` with the corners ordered (some writers store them
/// reversed); `None` if it isn't four numbers.
fn normalized_rect(v: &[f32]) -> Option<[f32; 4]> {
    let [x0, y0, x1, y1] = four(v)?;
    Some([x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)])
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::float_cmp)]
mod tests {
    use super::{appearance_matrix, normalized_rect};

    #[test]
    fn identity_when_bbox_equals_rect() {
        // Our own annotations: BBox == Rect, identity Matrix → identity placement.
        let m = appearance_matrix([10.0, 20.0, 110.0, 70.0], [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], [10.0, 20.0, 110.0, 70.0])
            .unwrap();
        assert_eq!(m, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    }

    #[test]
    fn scales_and_translates_bbox_onto_rect() {
        // A 0..10 square BBox placed onto a 100..200 / 50..150 rect → 10× scale + shift.
        let m = appearance_matrix([0.0, 0.0, 10.0, 10.0], [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], [100.0, 50.0, 200.0, 150.0])
            .unwrap();
        assert_eq!(m, [10.0, 0.0, 0.0, 10.0, 100.0, 50.0]);
    }

    #[test]
    fn degenerate_bbox_is_skipped() {
        assert!(appearance_matrix([5.0, 5.0, 5.0, 5.0], [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 10.0, 10.0]).is_none());
    }

    #[test]
    fn rect_corners_are_ordered() {
        // Stored reversed (x1<x0) → normalized to ascending.
        assert_eq!(normalized_rect(&[110.0, 70.0, 10.0, 20.0]), Some([10.0, 20.0, 110.0, 70.0]));
    }
}
