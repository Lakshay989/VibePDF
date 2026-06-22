//! XFDF annotation interchange (P3.E1).
//!
//! SPEC: P3-ANN-010 — export every annotation to an XFDF (XML) sidecar and
//! import one back, compatible with Adobe Acrobat.
//!
//! Two halves:
//!   * [`annotations_to_xfdf`] walks the raw lopdf annotation dicts and emits an
//!     XFDF document with full per-subtype fidelity (geometry, colour, opacity,
//!     `/NM` identity, reply links).
//!   * [`import_xfdf`] parses that XFDF (a focused, dependency-free reader) and
//!     recreates each annotation by **reusing the canonical `cos::add_*` writers**
//!     — so an imported highlight is structurally identical to a drawn one (same
//!     `/AP`, same `/BBox`) — then patches back the original `/NM`, `/Contents`,
//!     `/T`, and timestamps so identity and reply threads survive the round-trip.
//!
//! FDF is deferred (E1b). XML parsing is hand-rolled over the XFDF subset we and
//! Acrobat emit rather than pulling in a parser dependency (see CLAUDE.md).

use std::collections::HashSet;

use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::error::CommandError;
use crate::pdf::cos::{
    add_free_text, add_ink, add_line, add_measure, add_polygon, add_reply, add_shape, add_stamp,
    add_text_markup, add_text_note,
};

const XFDF_HEADER: &str = concat!(
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
    "<xfdf xmlns=\"http://ns.adobe.com/xfdf/\" xml:space=\"preserve\">\n",
);

#[allow(clippy::needless_pass_by_value)]
fn cos_err(e: lopdf::Error) -> CommandError {
    CommandError::Internal(format!("xfdf: {e}"))
}

// ===========================================================================
// Export
// ===========================================================================

/// SPEC: P3-ANN-010 — serialize every supported annotation in `bytes` to an XFDF
/// document. Returns the XML plus the count of annotations written. Foreign
/// subtypes (`/Link`, `/Widget`, `/Popup`, …) are skipped, mirroring
/// [`crate::pdf::cos::read_annotations`].
pub fn annotations_to_xfdf(bytes: &[u8]) -> Result<(String, usize), CommandError> {
    let doc = Document::load_mem(bytes).map_err(cos_err)?;
    let mut out = String::from(XFDF_HEADER);
    out.push_str("<annots>\n");
    let mut count = 0usize;

    for (page_no, page_id) in doc.get_pages() {
        let page = (page_no - 1) as usize;
        let arr = match doc.get_dictionary(page_id).ok().and_then(|p| p.get(b"Annots").ok().cloned()) {
            Some(Object::Array(a)) => a,
            Some(Object::Reference(id)) => {
                doc.get_object(id).and_then(Object::as_array).cloned().unwrap_or_default()
            }
            _ => continue,
        };
        for obj in arr {
            let Ok(id) = obj.as_reference() else { continue };
            let Ok(dict) = doc.get_dictionary(id) else { continue };
            let Some(element) =
                dict.get(b"Subtype").and_then(Object::as_name).ok().and_then(subtype_to_element)
            else {
                continue;
            };
            out.push_str(&emit_annotation(&doc, dict, element, page));
            count += 1;
        }
    }

    out.push_str("</annots>\n</xfdf>\n");
    Ok((out, count))
}

/// Map an annotation `/Subtype` to its XFDF element name, or `None` for subtypes
/// we don't surface.
fn subtype_to_element(subtype: &[u8]) -> Option<&'static str> {
    match subtype {
        b"Highlight" => Some("highlight"),
        b"Underline" => Some("underline"),
        b"StrikeOut" => Some("strikeout"),
        b"Squiggly" => Some("squiggly"),
        b"Text" => Some("text"),
        b"FreeText" => Some("freetext"),
        b"Square" => Some("square"),
        b"Circle" => Some("circle"),
        b"Line" => Some("line"),
        b"PolyLine" => Some("polyline"),
        b"Polygon" => Some("polygon"),
        b"Ink" => Some("ink"),
        b"Stamp" => Some("stamp"),
        _ => None,
    }
}

/// Build one XFDF element (with attributes + children) for an annotation dict.
fn emit_annotation(doc: &Document, dict: &Dictionary, element: &str, page: usize) -> String {
    use std::fmt::Write as _;

    let mut attrs: Vec<(String, String)> = Vec::new();
    attrs.push(("page".into(), page.to_string()));
    attrs.push(("rect".into(), floats_csv(&read_nums(dict, b"Rect"))));
    if let Some(nm) = read_text(dict, b"NM") {
        attrs.push(("name".into(), nm));
    }
    if let Some(t) = read_text(dict, b"T") {
        attrs.push(("title".into(), t));
    }
    if let Some(ca) = read_num(dict.get(b"CA").ok()) {
        attrs.push(("opacity".into(), format!("{ca:.4}")));
    }
    if let Some(c) = read_color_hex(dict, b"C") {
        attrs.push(("color".into(), c));
    }
    if let Some(d) = read_text(dict, b"CreationDate") {
        attrs.push(("creationdate".into(), d));
    }
    if let Some(d) = read_text(dict, b"M") {
        attrs.push(("date".into(), d));
    }

    // Subtype-specific geometry + style.
    match element {
        "highlight" | "underline" | "strikeout" | "squiggly" => {
            attrs.push(("coords".into(), floats_csv(&read_nums(dict, b"QuadPoints"))));
        }
        "square" | "circle" => {
            push_interior(dict, &mut attrs);
            push_border_width(dict, &mut attrs);
        }
        "line" => {
            let l = read_nums(dict, b"L");
            if l.len() >= 4 {
                attrs.push(("start".into(), floats_csv(&l[0..2])));
                attrs.push(("end".into(), floats_csv(&l[2..4])));
            }
            push_line_endings(dict, &mut attrs);
            push_border_width(dict, &mut attrs);
            push_intent(dict, &mut attrs);
        }
        "polyline" | "polygon" => {
            attrs.push(("vertices".into(), floats_csv(&read_nums(dict, b"Vertices"))));
            push_interior(dict, &mut attrs);
            push_border_width(dict, &mut attrs);
            push_intent(dict, &mut attrs);
        }
        "ink" => {
            push_border_width(dict, &mut attrs);
        }
        "text" => {
            if let Ok(icon) = dict.get(b"Name").and_then(Object::as_name) {
                attrs.push(("icon".into(), String::from_utf8_lossy(icon).into_owned()));
            }
            if let Some(parent) = irt_name(doc, dict) {
                attrs.push(("inreplyto".into(), parent));
                attrs.push(("replyType".into(), "reply".into()));
            }
        }
        "stamp" => {
            if let Ok(icon) = dict.get(b"Name").and_then(Object::as_name) {
                attrs.push(("icon".into(), String::from_utf8_lossy(icon).into_owned()));
            }
        }
        // "freetext" styling round-trips via the contents + a defaultappearance
        // child, handled below; no extra attributes here.
        _ => {}
    }

    let mut s = String::new();
    let _ = write!(s, "<{element}");
    for (k, v) in &attrs {
        let _ = write!(s, " {k}=\"{}\"", xml_escape(v));
    }

    // Children: contents, ink gestures, free-text default appearance.
    let contents = read_text(dict, b"Contents").unwrap_or_default();
    let da = if element == "freetext" { read_text(dict, b"DA") } else { None };
    let inklist = element == "ink";
    if contents.is_empty() && da.is_none() && !inklist {
        let _ = writeln!(s, "/>");
        return s;
    }
    let _ = writeln!(s, ">");
    if !contents.is_empty() {
        let _ = writeln!(s, "<contents>{}</contents>", xml_escape(&contents));
    }
    if let Some(da) = da {
        let _ = writeln!(s, "<defaultappearance>{}</defaultappearance>", xml_escape(&da));
    }
    if inklist {
        let _ = writeln!(s, "<inklist>");
        for gesture in read_inklist(dict) {
            let _ = writeln!(s, "<gesture>{}</gesture>", floats_csv(&gesture));
        }
        let _ = writeln!(s, "</inklist>");
    }
    let _ = writeln!(s, "</{element}>");
    s
}

fn push_interior(dict: &Dictionary, attrs: &mut Vec<(String, String)>) {
    if let Some(ic) = read_color_hex(dict, b"IC") {
        attrs.push(("interior-color".into(), ic));
    }
}

fn push_border_width(dict: &Dictionary, attrs: &mut Vec<(String, String)>) {
    if let Some(w) = dict.get(b"BS").and_then(Object::as_dict).ok().and_then(|bs| read_num(bs.get(b"W").ok())) {
        attrs.push(("width".into(), format!("{w:.2}")));
    }
}

fn push_intent(dict: &Dictionary, attrs: &mut Vec<(String, String)>) {
    if let Ok(it) = dict.get(b"IT").and_then(Object::as_name) {
        attrs.push(("it".into(), String::from_utf8_lossy(it).into_owned()));
    }
}

fn push_line_endings(dict: &Dictionary, attrs: &mut Vec<(String, String)>) {
    if let Ok(le) = dict.get(b"LE").and_then(Object::as_array) {
        let name = |i: usize| {
            le.get(i).and_then(|o| o.as_name().ok()).map(|n| String::from_utf8_lossy(n).into_owned())
        };
        if let Some(h) = name(0) {
            attrs.push(("head".into(), h));
        }
        if let Some(t) = name(1) {
            attrs.push(("tail".into(), t));
        }
    }
}

/// The `/NM` of the annotation this one replies to (resolved from `/IRT`), the
/// XFDF `inreplyto` handle. `None` when the annotation isn't a reply.
fn irt_name(doc: &Document, dict: &Dictionary) -> Option<String> {
    let parent_ref = dict.get(b"IRT").ok()?.as_reference().ok()?;
    let parent = doc.get_dictionary(parent_ref).ok()?;
    read_text(parent, b"NM")
}

// ===========================================================================
// Reading helpers (raw dict → values)
// ===========================================================================

#[allow(clippy::cast_precision_loss)]
fn obj_to_f32(o: &Object) -> Option<f32> {
    match o {
        Object::Real(r) => Some(*r),
        Object::Integer(n) => Some(*n as f32),
        _ => None,
    }
}

fn read_num(o: Option<&Object>) -> Option<f32> {
    o.and_then(obj_to_f32)
}

/// A numeric array field (`/Rect`, `/QuadPoints`, `/Vertices`, `/L`) flattened.
fn read_nums(dict: &Dictionary, key: &[u8]) -> Vec<f32> {
    dict.get(key)
        .and_then(Object::as_array)
        .ok()
        .map(|a| a.iter().filter_map(obj_to_f32).collect())
        .unwrap_or_default()
}

/// Each sub-path of an `/InkList` as a flat `[x y x y …]` vector.
fn read_inklist(dict: &Dictionary) -> Vec<Vec<f32>> {
    dict.get(b"InkList")
        .and_then(Object::as_array)
        .ok()
        .map(|paths| {
            paths
                .iter()
                .filter_map(|p| p.as_array().ok())
                .map(|g| g.iter().filter_map(obj_to_f32).collect())
                .collect()
        })
        .unwrap_or_default()
}

fn read_text(dict: &Dictionary, key: &[u8]) -> Option<String> {
    let s = dict.get(key).and_then(Object::as_str).ok()?;
    Some(String::from_utf8_lossy(s).into_owned())
}

/// An `/C` or `/IC` colour array → `#rrggbb`. Handles RGB (3) and gray (1);
/// other component counts (e.g. CMYK) yield `None`.
fn read_color_hex(dict: &Dictionary, key: &[u8]) -> Option<String> {
    let a = dict.get(key).and_then(Object::as_array).ok()?;
    let nums: Vec<f32> = a.iter().filter_map(obj_to_f32).collect();
    match nums.as_slice() {
        [r, g, b] => Some(rgb_hex(*r, *g, *b)),
        [gray] => Some(rgb_hex(*gray, *gray, *gray)),
        _ => None,
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn rgb_hex(r: f32, g: f32, b: f32) -> String {
    let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", c(r), c(g), c(b))
}

fn floats_csv(v: &[f32]) -> String {
    v.iter().map(|x| format!("{x:.2}")).collect::<Vec<_>>().join(",")
}

fn xml_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            _ => o.push(c),
        }
    }
    o
}

// ===========================================================================
// Import
// ===========================================================================

/// SPEC: P3-ANN-010 — parse `xfdf` and add each annotation it describes to
/// `bytes`, returning the new document. Non-reply annotations are added first
/// (preserving each `/NM`), then replies are wired to their parents by `/IRT`.
/// Out-of-range pages and orphan replies (parent absent) are skipped, not fatal.
pub fn import_xfdf(bytes: &[u8], xfdf: &str) -> Result<Vec<u8>, CommandError> {
    let root = parse_xml(xfdf)?;
    let parsed = collect_annots(&root);

    let mut cur = bytes.to_vec();
    let mut known: HashSet<ObjectId> = annotation_ids(&cur)?;
    let mut present_names: HashSet<String> = name_set(&cur)?;

    // Pass 1 — every non-reply annotation, preserving identity.
    let (replies, tops): (Vec<&ParsedAnnot>, Vec<&ParsedAnnot>) =
        parsed.iter().partition(|a| a.in_reply_to.is_some());
    for el in tops {
        match add_top_level(&cur, el) {
            Ok(next) => {
                cur = patch_identity(&cur, next, &known, el)?;
                known = annotation_ids(&cur)?;
                if let Some(name) = &el.name {
                    present_names.insert(name.clone());
                }
            }
            // A single malformed/out-of-range annotation must not abort the import.
            Err(CommandError::InvalidInput(_)) => {}
            Err(e) => return Err(e),
        }
    }

    // Pass 2 — replies, resolvable once their parent exists. Fixed-point so a
    // reply-to-a-reply lands after its parent reply.
    let mut pending: Vec<&ParsedAnnot> = replies;
    loop {
        let mut progressed = false;
        let mut still: Vec<&ParsedAnnot> = Vec::new();
        for el in pending {
            let parent = el.in_reply_to.as_deref().unwrap_or_default();
            if !present_names.contains(parent) {
                still.push(el);
                continue;
            }
            match add_reply(&cur, parent, &el.author, &el.contents) {
                Ok(next) => {
                    cur = patch_identity(&cur, next, &known, el)?;
                    known = annotation_ids(&cur)?;
                    if let Some(name) = &el.name {
                        present_names.insert(name.clone());
                    }
                    progressed = true;
                }
                Err(CommandError::InvalidInput(_)) => {}
                Err(e) => return Err(e),
            }
        }
        pending = still;
        if pending.is_empty() || !progressed {
            break;
        }
    }

    Ok(cur)
}

/// Dispatch one parsed (non-reply) annotation to the canonical `cos::add_*`
/// writer for its element. Geometry/colour come from the XFDF; identity is
/// patched afterwards.
fn add_top_level(bytes: &[u8], el: &ParsedAnnot) -> Result<Vec<u8>, CommandError> {
    let page = el.page;
    let color = el.color.clone().unwrap_or_else(|| default_color(&el.element));
    let opacity = el.opacity.unwrap_or(1.0);
    let width = el.width.unwrap_or(1.0);
    match el.element.as_str() {
        "highlight" | "underline" | "strikeout" | "squiggly" => {
            let subtype = if el.element == "strikeout" { "strikethrough" } else { &el.element };
            let quads = chunk8(&el.coords);
            add_text_markup(bytes, page, subtype, &quads, &color, opacity)
        }
        "square" => add_shape(bytes, page, "rectangle", el.rect, &color, el.interior.as_deref(), opacity, width),
        "circle" => add_shape(bytes, page, "ellipse", el.rect, &color, el.interior.as_deref(), opacity, width),
        "line" if el.it.is_some() => {
            add_measure(bytes, page, "distance", &chunk2(&el.line), &color, &el.contents, opacity, width)
        }
        "line" => {
            let [x1, y1, x2, y2] = four(&el.line, el.rect);
            add_line(bytes, page, x1, y1, x2, y2, el.arrow, &color, opacity, width)
        }
        "polyline" if el.it.is_some() => {
            add_measure(bytes, page, "perimeter", &chunk2(&el.vertices), &color, &el.contents, opacity, width)
        }
        "polygon" if el.it.is_some() => {
            add_measure(bytes, page, "area", &chunk2(&el.vertices), &color, &el.contents, opacity, width)
        }
        "polyline" => add_polygon(bytes, page, false, &chunk2(&el.vertices), &color, None, opacity, width),
        "polygon" => {
            add_polygon(bytes, page, true, &chunk2(&el.vertices), &color, el.interior.as_deref(), opacity, width)
        }
        "ink" => {
            let pts: Vec<[f32; 3]> = chunk2(&el.gesture).iter().map(|[x, y]| [*x, *y, 0.5]).collect();
            add_ink(bytes, page, &pts, &color, opacity, width.max(0.5))
        }
        "stamp" => {
            let label = if el.contents.is_empty() { "STAMP" } else { &el.contents };
            let name = el.icon.clone().unwrap_or_else(|| "Draft".into());
            add_stamp(bytes, page, el.rect, label, &name, &color, opacity)
        }
        "freetext" => {
            let (size, fcolor) = parse_default_appearance(el.default_appearance.as_deref(), &color);
            add_free_text(bytes, page, el.rect, &el.contents, "Helvetica", size, &fcolor, false, false)
        }
        "text" => {
            let name = el.name.clone().unwrap_or_else(new_uuid);
            let [x, y, _, _] = el.rect;
            add_text_note(bytes, &name, page, x, y, &el.contents, &el.author)
        }
        other => Err(CommandError::InvalidInput(format!("unknown xfdf element: {other}"))),
    }
}

/// Set the freshly-added annotation's `/NM`, `/Contents`, `/T`, and timestamps
/// from the XFDF, so identity + reply links survive. The new annotation is the
/// single `/Annots` entry not already in `known`.
fn patch_identity(
    _prev: &[u8],
    new_bytes: Vec<u8>,
    known: &HashSet<ObjectId>,
    el: &ParsedAnnot,
) -> Result<Vec<u8>, CommandError> {
    let mut doc = Document::load_mem(&new_bytes).map_err(cos_err)?;
    let after = annotation_ids_doc(&doc);
    let Some(new_id) = after.difference(known).next().copied() else {
        // Nothing new (e.g. add was a silent no-op) — leave bytes as-is.
        return Ok(new_bytes);
    };
    let dict = doc.get_dictionary_mut(new_id).map_err(cos_err)?;
    if let Some(name) = &el.name {
        dict.set("NM", Object::string_literal(name.clone()));
    }
    if !el.contents.is_empty() {
        dict.set("Contents", Object::string_literal(el.contents.clone()));
    }
    if !el.author.is_empty() {
        dict.set("T", Object::string_literal(el.author.clone()));
    }
    if let Some(d) = &el.creation_date {
        dict.set("CreationDate", Object::string_literal(d.clone()));
    }
    if let Some(d) = &el.modified_date {
        dict.set("M", Object::string_literal(d.clone()));
    }
    let mut buf = Vec::new();
    doc.save_to(&mut buf).map_err(CommandError::from)?;
    Ok(buf)
}

fn new_uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn default_color(element: &str) -> String {
    match element {
        "highlight" => "#ffff00".into(),
        _ => "#000000".into(),
    }
}

/// Reconstruct `[x1,y1,x2,y2]` for a line from its `start`/`end` (`/L`) values,
/// falling back to the rect diagonal if absent.
fn four(line: &[f32], rect: [f32; 4]) -> [f32; 4] {
    if line.len() >= 4 {
        [line[0], line[1], line[2], line[3]]
    } else {
        rect
    }
}

fn chunk2(v: &[f32]) -> Vec<[f32; 2]> {
    v.chunks_exact(2).map(|c| [c[0], c[1]]).collect()
}

fn chunk8(v: &[f32]) -> Vec<[f32; 8]> {
    v.chunks_exact(8).map(|c| [c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7]]).collect()
}

/// Parse `size` + colour out of a free-text `/DA` string (`r g b rg /F1 size Tf`),
/// falling back to 12pt and `fallback` colour.
fn parse_default_appearance(da: Option<&str>, fallback: &str) -> (f32, String) {
    let Some(da) = da else { return (12.0, fallback.to_string()) };
    let toks: Vec<&str> = da.split_whitespace().collect();
    let mut size = 12.0;
    let mut color = fallback.to_string();
    for w in toks.windows(2) {
        if w[1] == "Tf" {
            if let Ok(s) = w[0].parse::<f32>() {
                if s > 0.0 {
                    size = s;
                }
            }
        }
    }
    // `r g b rg` colour operator.
    for i in 0..toks.len() {
        if toks[i] == "rg" && i >= 3 {
            let p = |j: usize| toks[i - 3 + j].parse::<f32>().ok();
            if let (Some(r), Some(g), Some(b)) = (p(0), p(1), p(2)) {
                color = rgb_hex(r, g, b);
            }
        }
    }
    (size, color)
}

// --- annotation id / name bookkeeping over serialized bytes ---

fn annotation_ids(bytes: &[u8]) -> Result<HashSet<ObjectId>, CommandError> {
    let doc = Document::load_mem(bytes).map_err(cos_err)?;
    Ok(annotation_ids_doc(&doc))
}

fn annotation_ids_doc(doc: &Document) -> HashSet<ObjectId> {
    let mut ids = HashSet::new();
    for page_id in doc.get_pages().values() {
        let arr = match doc.get_dictionary(*page_id).ok().and_then(|p| p.get(b"Annots").ok().cloned()) {
            Some(Object::Array(a)) => a,
            Some(Object::Reference(id)) => {
                doc.get_object(id).and_then(Object::as_array).cloned().unwrap_or_default()
            }
            _ => continue,
        };
        for obj in arr {
            if let Ok(id) = obj.as_reference() {
                ids.insert(id);
            }
        }
    }
    ids
}

fn name_set(bytes: &[u8]) -> Result<HashSet<String>, CommandError> {
    let doc = Document::load_mem(bytes).map_err(cos_err)?;
    let mut names = HashSet::new();
    for id in annotation_ids_doc(&doc) {
        if let Some(nm) = doc.get_dictionary(id).ok().and_then(|d| read_text(d, b"NM")) {
            names.insert(nm);
        }
    }
    Ok(names)
}

// ===========================================================================
// Parsed model + XFDF → model
// ===========================================================================

/// One annotation lifted from the XFDF, normalized for [`add_top_level`].
#[derive(Debug, Default)]
struct ParsedAnnot {
    element: String,
    page: usize,
    rect: [f32; 4],
    color: Option<String>,
    interior: Option<String>,
    opacity: Option<f32>,
    width: Option<f32>,
    coords: Vec<f32>,
    vertices: Vec<f32>,
    line: Vec<f32>,
    gesture: Vec<f32>,
    arrow: bool,
    it: Option<String>,
    icon: Option<String>,
    name: Option<String>,
    author: String,
    contents: String,
    creation_date: Option<String>,
    modified_date: Option<String>,
    in_reply_to: Option<String>,
    default_appearance: Option<String>,
}

const ELEMENTS: &[&str] = &[
    "highlight", "underline", "strikeout", "squiggly", "text", "freetext", "square", "circle",
    "line", "polyline", "polygon", "ink", "stamp",
];

/// Walk the parsed XML for `<annots>` and turn each known child element into a
/// [`ParsedAnnot`].
fn collect_annots(root: &XmlNode) -> Vec<ParsedAnnot> {
    let Some(annots) = root.find("annots") else { return Vec::new() };
    annots
        .children
        .iter()
        .filter(|c| ELEMENTS.contains(&c.name.as_str()))
        .map(parse_annot_node)
        .collect()
}

fn parse_annot_node(node: &XmlNode) -> ParsedAnnot {
    let attr = |k: &str| node.attr(k);
    let rect = {
        let v = parse_floats(&attr("rect").unwrap_or_default());
        four(&v, [0.0; 4])
    };
    ParsedAnnot {
        element: node.name.clone(),
        page: attr("page").and_then(|s| s.trim().parse::<usize>().ok()).unwrap_or(0),
        rect,
        color: attr("color"),
        interior: attr("interior-color"),
        opacity: attr("opacity").and_then(|s| s.trim().parse::<f32>().ok()),
        width: attr("width").and_then(|s| s.trim().parse::<f32>().ok()),
        coords: parse_floats(&attr("coords").unwrap_or_default()),
        vertices: parse_floats(&attr("vertices").unwrap_or_default()),
        line: {
            let mut l = parse_floats(&attr("start").unwrap_or_default());
            l.extend(parse_floats(&attr("end").unwrap_or_default()));
            l
        },
        gesture: node
            .find("inklist")
            .and_then(|il| il.find("gesture"))
            .map(|g| parse_floats(&g.text))
            .unwrap_or_default(),
        arrow: line_has_arrow(attr("head").as_deref(), attr("tail").as_deref()),
        it: attr("it"),
        icon: attr("icon"),
        name: attr("name"),
        author: attr("title").unwrap_or_default(),
        contents: node.find("contents").map(|c| c.text.clone()).unwrap_or_default(),
        creation_date: attr("creationdate"),
        modified_date: attr("date"),
        in_reply_to: attr("inreplyto"),
        default_appearance: node.find("defaultappearance").map(|c| c.text.clone()),
    }
}

fn line_has_arrow(head: Option<&str>, tail: Option<&str>) -> bool {
    let is_arrow = |e: Option<&str>| matches!(e, Some(s) if !s.eq_ignore_ascii_case("None") && !s.is_empty());
    is_arrow(head) || is_arrow(tail)
}

/// Split a numeric attribute (`coords`, `vertices`, `start`, …) into floats,
/// lenient about the separator (comma, semicolon, or whitespace) so both our
/// output and Acrobat's are accepted.
fn parse_floats(s: &str) -> Vec<f32> {
    s.split([',', ';', ' ', '\t', '\n', '\r'])
        .filter(|t| !t.is_empty())
        .filter_map(|t| t.parse::<f32>().ok())
        .collect()
}

// ===========================================================================
// Minimal XML reader (the XFDF subset)
// ===========================================================================

/// A parsed XML element: tag name, attributes, child elements, and the
/// concatenated text of its direct text nodes (entity-decoded).
#[derive(Debug, Default)]
struct XmlNode {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<XmlNode>,
    text: String,
}

impl XmlNode {
    fn attr(&self, key: &str) -> Option<String> {
        self.attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    /// The first descendant (depth-first) with the given tag name.
    fn find(&self, name: &str) -> Option<&XmlNode> {
        for c in &self.children {
            if c.name == name {
                return Some(c);
            }
            if let Some(found) = c.find(name) {
                return Some(found);
            }
        }
        None
    }
}

/// Parse an XFDF/XML string into its root element. Tolerant of a leading XML
/// declaration, comments, and processing instructions; rejects clearly
/// malformed input rather than panicking.
fn parse_xml(input: &str) -> Result<XmlNode, CommandError> {
    let chars: Vec<char> = input.chars().collect();
    let mut pos = 0usize;
    skip_prolog(&chars, &mut pos);
    let node = parse_element(&chars, &mut pos)
        .ok_or_else(|| CommandError::InvalidInput("malformed XFDF: no root element".into()))?;
    Ok(node)
}

/// Advance past whitespace, `<?…?>` declarations/PIs, `<!-- … -->` comments, and
/// `<!DOCTYPE …>` until the first real element's `<`.
fn skip_prolog(chars: &[char], pos: &mut usize) {
    loop {
        skip_ws(chars, pos);
        if peek2(chars, *pos) == Some(['<', '?']) {
            skip_until(chars, pos, "?>");
        } else if starts_with(chars, *pos, "<!--") {
            skip_until(chars, pos, "-->");
        } else if starts_with(chars, *pos, "<!") {
            skip_until(chars, pos, ">");
        } else {
            break;
        }
    }
}

/// Parse one element beginning at `chars[pos] == '<'`, recursively reading its
/// children. Returns `None` on malformed structure.
fn parse_element(chars: &[char], pos: &mut usize) -> Option<XmlNode> {
    skip_ws(chars, pos);
    if chars.get(*pos) != Some(&'<') {
        return None;
    }
    *pos += 1; // consume '<'
    let name = read_name(chars, pos);
    if name.is_empty() {
        return None;
    }
    let mut node = XmlNode { name, ..XmlNode::default() };

    // Attributes.
    loop {
        skip_ws(chars, pos);
        match chars.get(*pos) {
            Some('/') if chars.get(*pos + 1) == Some(&'>') => {
                *pos += 2; // self-closing
                return Some(node);
            }
            Some('>') => {
                *pos += 1;
                break;
            }
            Some(_) => {
                let key = read_name(chars, pos);
                if key.is_empty() {
                    *pos += 1; // skip a stray char to guarantee progress
                    continue;
                }
                skip_ws(chars, pos);
                let mut value = String::new();
                if chars.get(*pos) == Some(&'=') {
                    *pos += 1;
                    skip_ws(chars, pos);
                    value = read_quoted(chars, pos);
                }
                node.attrs.push((key, value));
            }
            None => return None,
        }
    }

    // Content: text + child elements until the matching close tag.
    loop {
        match chars.get(*pos) {
            None => break, // unbalanced — accept what we have
            Some('<') => {
                if chars.get(*pos + 1) == Some(&'/') {
                    // Closing tag — consume `</name>`.
                    skip_until(chars, pos, ">");
                    break;
                } else if starts_with(chars, *pos, "<!--") {
                    skip_until(chars, pos, "-->");
                } else if starts_with(chars, *pos, "<![CDATA[") {
                    *pos += "<![CDATA[".chars().count();
                    let start = *pos;
                    skip_until(chars, pos, "]]>");
                    let end = pos.saturating_sub(3).max(start);
                    node.text.extend(&chars[start..end]);
                } else if let Some(child) = parse_element(chars, pos) {
                    node.children.push(child);
                } else {
                    *pos += 1; // guarantee progress on garbage
                }
            }
            Some(_) => {
                let start = *pos;
                while *pos < chars.len() && chars[*pos] != '<' {
                    *pos += 1;
                }
                node.text.push_str(&decode_entities(&chars[start..*pos]));
            }
        }
    }
    Some(node)
}

fn read_name(chars: &[char], pos: &mut usize) -> String {
    skip_ws(chars, pos);
    let start = *pos;
    while let Some(&c) = chars.get(*pos) {
        if c.is_alphanumeric() || matches!(c, '_' | '-' | ':' | '.') {
            *pos += 1;
        } else {
            break;
        }
    }
    chars[start..*pos].iter().collect()
}

fn read_quoted(chars: &[char], pos: &mut usize) -> String {
    let Some(&quote @ ('"' | '\'')) = chars.get(*pos) else {
        return String::new();
    };
    *pos += 1;
    let start = *pos;
    while let Some(&c) = chars.get(*pos) {
        if c == quote {
            break;
        }
        *pos += 1;
    }
    let value = decode_entities(&chars[start..*pos]);
    if chars.get(*pos) == Some(&quote) {
        *pos += 1;
    }
    value
}

fn decode_entities(chars: &[char]) -> String {
    let mut out = String::with_capacity(chars.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '&' {
            if let Some(semi) = chars[i..].iter().position(|&c| c == ';') {
                let entity: String = chars[i + 1..i + semi].iter().collect();
                if let Some(ch) = entity_char(&entity) {
                    out.push(ch);
                    i += semi + 1;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn entity_char(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => {
            let code = entity.strip_prefix('#')?;
            let n = if let Some(hex) = code.strip_prefix(['x', 'X']) {
                u32::from_str_radix(hex, 16).ok()?
            } else {
                code.parse::<u32>().ok()?
            };
            char::from_u32(n)
        }
    }
}

fn skip_ws(chars: &[char], pos: &mut usize) {
    while let Some(&c) = chars.get(*pos) {
        if c.is_whitespace() {
            *pos += 1;
        } else {
            break;
        }
    }
}

fn peek2(chars: &[char], pos: usize) -> Option<[char; 2]> {
    Some([*chars.get(pos)?, *chars.get(pos + 1)?])
}

fn starts_with(chars: &[char], pos: usize, pat: &str) -> bool {
    pat.chars().enumerate().all(|(i, c)| chars.get(pos + i) == Some(&c))
}

/// Advance `pos` to just past the next occurrence of `pat` (or to the end).
fn skip_until(chars: &[char], pos: &mut usize, pat: &str) {
    let pat: Vec<char> = pat.chars().collect();
    while *pos < chars.len() {
        if starts_with(chars, *pos, &pat.iter().collect::<String>()) {
            *pos += pat.len();
            return;
        }
        *pos += 1;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::{
        collect_annots, parse_default_appearance, parse_floats, parse_xml, subtype_to_element,
        xml_escape,
    };

    #[test]
    fn escapes_xml_entities() {
        assert_eq!(
            xml_escape("a & b < c > d \" e ' f"),
            "a &amp; b &lt; c &gt; d &quot; e &apos; f"
        );
    }

    #[test]
    fn decodes_named_and_numeric_entities() {
        let xml = r#"<xfdf><annots><text page="0" rect="0,0,1,1" title="A&amp;B"><contents>x &lt; y &#65; &#x42;</contents></text></annots></xfdf>"#;
        let root = parse_xml(xml).expect("parse");
        let a = collect_annots(&root);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].author, "A&B", "attribute entities decode");
        assert_eq!(a[0].contents, "x < y A B", "text + numeric entities decode");
    }

    #[test]
    fn parse_floats_is_separator_lenient() {
        assert_eq!(parse_floats("1,2 ;3\t4"), vec![1.0, 2.0, 3.0, 4.0]);
        assert_eq!(parse_floats(""), Vec::<f32>::new());
        assert_eq!(parse_floats("1,foo,2"), vec![1.0, 2.0], "non-numeric tokens are dropped");
    }

    #[test]
    fn collects_known_elements_only() {
        let xml = r##"<xfdf><annots>
            <highlight page="1" rect="10,20,30,40" coords="10,40,30,40,10,20,30,20" color="#ffff00"/>
            <square page="0" rect="1,2,3,4" interior-color="#00ff00" width="2"/>
            <line page="0" start="0,0" end="5,5" head="None" tail="OpenArrow" it="LineDimension"/>
            <ink page="0"><inklist><gesture>1,1;2,2;3,1</gesture></inklist></ink>
            <text page="0" rect="0,0,1,1" inreplyto="root" title="Bo"><contents>hi</contents></text>
            <bogus page="0"/>
        </annots></xfdf>"##;
        let root = parse_xml(xml).expect("parse");
        let a = collect_annots(&root);
        assert_eq!(a.len(), 5, "the <bogus> element is skipped");

        assert_eq!(a[0].element, "highlight");
        assert_eq!(a[0].page, 1);
        assert_eq!(a[0].coords.len(), 8);

        assert_eq!(a[1].interior.as_deref(), Some("#00ff00"));
        assert_eq!(a[1].width, Some(2.0));

        assert_eq!(a[2].line, vec![0.0, 0.0, 5.0, 5.0]);
        assert!(a[2].arrow, "tail=OpenArrow means an arrow");
        assert_eq!(a[2].it.as_deref(), Some("LineDimension"));

        assert_eq!(a[3].gesture, vec![1.0, 1.0, 2.0, 2.0, 3.0, 1.0]);

        assert_eq!(a[4].in_reply_to.as_deref(), Some("root"));
        assert_eq!(a[4].contents, "hi");
    }

    #[test]
    fn tolerates_prolog_comments_and_self_closing() {
        let xml = "<?xml version=\"1.0\"?>\n<!-- a comment -->\n<xfdf xmlns=\"http://ns.adobe.com/xfdf/\"><annots><square page=\"0\" rect=\"0,0,1,1\"/></annots></xfdf>";
        let root = parse_xml(xml).expect("parse");
        assert_eq!(collect_annots(&root).len(), 1);
    }

    #[test]
    fn malformed_input_does_not_panic() {
        for s in ["", "not xml", "<xfdf><annots><highlight ", "<<<>>>", "<a><b></a>"] {
            let _ = parse_xml(s).map(|r| collect_annots(&r));
        }
    }

    #[test]
    fn subtype_mapping_round_trips_known_kinds() {
        assert_eq!(subtype_to_element(b"Highlight"), Some("highlight"));
        assert_eq!(subtype_to_element(b"StrikeOut"), Some("strikeout"));
        assert_eq!(subtype_to_element(b"Link"), None, "foreign subtypes are skipped");
    }

    #[test]
    fn parses_default_appearance_size_and_color() {
        let (size, color) = parse_default_appearance(Some("0 0 1 rg /F1 14 Tf"), "#000000");
        assert!((size - 14.0).abs() < f32::EPSILON);
        assert_eq!(color, "#0000ff");
    }
}
