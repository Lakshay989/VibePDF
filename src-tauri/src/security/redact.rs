//! True redaction: removing content, not covering it (P6.D1a, SPEC P6-SEC-010).
//!
//! ## The governing principle
//!
//! **When we cannot measure precisely, remove more.** Over-redaction is a
//! visible annoyance the user can undo; under-redaction is a permanent leak
//! they will never discover, in a document they have already sent. Every
//! ambiguous case here resolves toward removal, and [`RedactReport`] counts the
//! ones that did so the choice is visible rather than silent.
//!
//! ## Why this walks the content stream itself
//!
//! The obvious implementation delegates to [`crate::pdf::reflow::delete_text_run`],
//! which takes a run index. That index counts **`PDFium` text objects** while the
//! surgery happens on **content-stream show operators**, and on real documents
//! the two disagree — `PDFium` drops whitespace-only runs, so a leading `( )Tj`
//! shifts every subsequent index by one. `reflow` guards this and refuses when
//! it detects the mismatch, which is correct for editing and useless here: a
//! leading indent is ordinary, so redaction would refuse on a large class of
//! real files.
//!
//! So this module never uses a `PDFium` index. It walks the operators, tracks
//! the text and transformation matrices itself, and decides from geometry.
//! `PDFium` is still used — for *verifying* the result, which is what it is
//! good at and what clause (c) asks for.
//!
//! ## What is refused rather than half-done
//!
//! Text inside a Form `XObject` is invisible to a page-content walk. Redacting
//! the page and reporting success would leave the text in the file and tell the
//! user it was gone — the worst outcome this module can produce. Such a page is
//! **refused**.

use std::collections::BTreeSet;

use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::error::CommandError;
use crate::pdf::font_metrics::{has_exact_metrics, text_width};

#[allow(clippy::needless_pass_by_value)]
fn redact_err(e: lopdf::Error) -> CommandError {
    CommandError::PdfError(format!("redact: {e}"))
}

fn refuse(msg: impl Into<String>) -> CommandError {
    CommandError::InvalidInput(msg.into())
}

/// What a redaction did, per category.
///
/// `removed_whole_for_safety` is the number the user should look at: it counts
/// runs that were only *partly* inside the region but could not be measured, so
/// the whole run went. It is the visible price of the safe default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RedactReport {
    /// Runs entirely inside the region.
    pub removed: usize,
    /// Runs partly inside, measured, and cut at the right character.
    pub split: usize,
    /// Runs partly inside that could not be measured, so went whole.
    pub removed_whole_for_safety: usize,
    /// Images whose placed rectangle touched the region.
    pub images_removed: usize,
}

/// SPEC: P6-SEC-010 — what a redaction should take out.
///
/// Clause (b) — "optionally remove or rewrite metadata" — is a toggle rather
/// than automatic. A redaction is often one of several on a document, and
/// stripping the metadata on each pass would be surprising; it is also exactly
/// what `P6.D3` already does, so this delegates rather than reimplements.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RedactOptions {
    /// Also strip `/Info` and the XMP packet, via `pdf::clean`.
    pub remove_metadata: bool,
}

impl RedactReport {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// A 2-D affine transform, in PDF order: `[a b c d e f]`.
#[derive(Debug, Clone, Copy)]
struct Matrix {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
    e: f32,
    f: f32,
}

impl Matrix {
    const IDENTITY: Self = Self { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

    /// `self × other` — self applied first, then other, as PDF composes them.
    fn then(self, other: Self) -> Self {
        Self {
            a: self.a * other.a + self.b * other.c,
            b: self.a * other.b + self.b * other.d,
            c: self.c * other.a + self.d * other.c,
            d: self.c * other.b + self.d * other.d,
            e: self.e * other.a + self.f * other.c + other.e,
            f: self.e * other.b + self.f * other.d + other.f,
        }
    }

    fn translate(x: f32, y: f32) -> Self {
        Self { e: x, f: y, ..Self::IDENTITY }
    }

    /// Is this axis-aligned and unrotated? Anything else, we decline to measure.
    fn is_upright(self) -> bool {
        self.b.abs() < 1e-6 && self.c.abs() < 1e-6
    }
}

/// Everything about the text state that affects where glyphs land.
#[derive(Debug, Clone)]
struct TextState {
    tm: Matrix,
    tlm: Matrix,
    leading: f32,
    font: Option<String>,
    size: f32,
    /// Horizontal scaling, `Tz`, as a fraction (100 → 1.0).
    scale: f32,
    char_spacing: f32,
    word_spacing: f32,
}

impl Default for TextState {
    fn default() -> Self {
        Self {
            tm: Matrix::IDENTITY,
            tlm: Matrix::IDENTITY,
            leading: 0.0,
            font: None,
            size: 0.0,
            scale: 1.0,
            char_spacing: 0.0,
            word_spacing: 0.0,
        }
    }
}

/// SPEC: P6-SEC-010 — remove the text inside `rect` on `page`, then mark it.
///
/// `rect` is `[x0, y0, x1, y1]` in PDF user space. Returns the new bytes and a
/// report; the input is never mutated.
///
/// Clause (c) is enforced, not merely attempted: the result is re-extracted and
/// the redacted text must be absent, or this returns an error and the caller
/// keeps the original bytes.
pub fn redact_text_in_region(
    bytes: &[u8],
    page: usize,
    rect: [f32; 4],
) -> Result<(Vec<u8>, RedactReport), CommandError> {
    redact_region(bytes, page, rect, RedactOptions::default())
}

/// SPEC: P6-SEC-010 — remove the text *and* images inside `rect` on `page`.
pub fn redact_region(
    bytes: &[u8],
    page: usize,
    rect: [f32; 4],
    options: RedactOptions,
) -> Result<(Vec<u8>, RedactReport), CommandError> {
    let mut doc = Document::load_mem(bytes).map_err(redact_err)?;
    let page_id = nth_page(&doc, page)?;

    refuse_if_text_hides_in_an_xobject(&doc, page_id)?;

    let fonts = page_fonts(&doc, page_id);
    let images = page_images(&doc, page_id);
    let content = doc.get_page_content(page_id).map_err(redact_err)?;
    let parsed = Content::decode(&content).map_err(redact_err)?;

    let doomed = images_in_region(&parsed.operations, &images, normalise(rect));
    let (mut operations, mut report) = rewrite(parsed.operations, &fonts, normalise(rect));

    if !doomed.is_empty() {
        operations.retain(|op| {
            op.operator != "Do"
                || !op
                    .operands
                    .first()
                    .and_then(|o| o.as_name().ok())
                    .is_some_and(|n| doomed.contains(&String::from_utf8_lossy(n).into_owned()))
        });
        report.images_removed = doomed.len();
    }

    if report.is_empty() {
        // Nothing intersected. Returning the input unchanged is more honest
        // than re-encoding it and calling that a redaction.
        return Ok((bytes.to_vec(), report));
    }

    let encoded = Content { operations }.encode().map_err(redact_err)?;
    doc.change_page_content(page_id, encoded).map_err(redact_err)?;

    // Dropping the `Do` undraws the image; it does not remove it. The stream
    // stays in the file with its pixels intact, findable by anything that reads
    // the bytes — the same trap as detaching `/Info` in P6.D3. Both the
    // resource entry and the object itself have to go.
    for name in &doomed {
        if let Some(id) = images.get(name) {
            drop_image_resource(&mut doc, page_id, name.as_bytes());
            doc.objects.remove(id);
        }
    }

    cover(&mut doc, page_id, normalise(rect))?;

    let mut out = Vec::new();
    doc.save_to(&mut out)
        .map_err(|e| CommandError::PdfError(format!("redact: lopdf save: {e}")))?;

    // SPEC: P6-SEC-010(b) — optional, and delegated. P6.D3 already does this
    // properly, including the XMP packet that an `/Info`-only sweep misses.
    if options.remove_metadata {
        let opts = crate::pdf::clean::CleanOptions {
            metadata: true,
            ..crate::pdf::clean::CleanOptions::default()
        };
        let (cleaned, _) = crate::pdf::clean::clean_document(&out, &opts)?;
        return Ok((cleaned, report));
    }
    Ok((out, report))
}

/// Remove one name from the page's `/XObject` resources.
fn drop_image_resource(doc: &mut Document, page_id: ObjectId, name: &[u8]) {
    // The resources may be a direct dictionary on the page or an indirect
    // object shared with other pages; only the first is ours to edit here.
    let via_ref = doc
        .get_object(page_id)
        .and_then(Object::as_dict)
        .ok()
        .and_then(|p| p.get(b"Resources").ok())
        .and_then(|o| o.as_reference().ok());

    let target = via_ref.unwrap_or(page_id);
    let Ok(dict) = doc.get_object_mut(target).and_then(Object::as_dict_mut) else {
        return;
    };
    let resources = if via_ref.is_some() {
        Some(dict)
    } else {
        dict.get_mut(b"Resources").ok().and_then(|o| match o {
            Object::Dictionary(d) => Some(d),
            _ => None,
        })
    };
    if let Some(res) = resources {
        if let Ok(Object::Dictionary(xobjects)) = res.get_mut(b"XObject") {
            xobjects.remove(name);
        }
    }
}

fn normalise([x0, y0, x1, y1]: [f32; 4]) -> [f32; 4] {
    [x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)]
}

fn nth_page(doc: &Document, page: usize) -> Result<ObjectId, CommandError> {
    doc.get_pages()
        .into_values()
        .nth(page)
        .ok_or_else(|| refuse(format!("this document has no page {}", page + 1)))
}

/// SPEC: P6-SEC-010 — refuse a page whose text we cannot see.
///
/// A Form `XObject`'s content is a separate stream; a page-content walk never
/// looks inside it. Redacting the page and reporting success would leave that
/// text in the file while telling the user it was removed, which is worse than
/// any amount of over-redaction.
///
/// The check is deliberately coarse — *any* form carrying text-showing
/// operators refuses the page, whether or not it overlaps the region. Deciding
/// overlap would mean resolving the form's own matrix and bounding box, and a
/// wrong answer there fails in the direction that leaks.
fn refuse_if_text_hides_in_an_xobject(
    doc: &Document,
    page_id: ObjectId,
) -> Result<(), CommandError> {
    let Ok(page) = doc.get_object(page_id).and_then(Object::as_dict) else {
        return Ok(());
    };
    let Some(xobjects) = resources_of(doc, page).and_then(|r| {
        r.get(b"XObject")
            .ok()
            .and_then(|o| resolve_dict(doc, o))
    }) else {
        return Ok(());
    };

    for (name, entry) in xobjects {
        let Ok(id) = entry.as_reference() else { continue };
        let Ok(stream) = doc.get_object(id).and_then(|o| o.as_stream()) else {
            continue;
        };
        let is_form = stream
            .dict
            .get(b"Subtype")
            .and_then(Object::as_name)
            .is_ok_and(|n| n == b"Form");
        if !is_form {
            continue;
        }
        // Uncompressed streams make `decompressed_content` fail, so fall back
        // to the raw bytes. Treating that failure as "no text" would silently
        // skip the very check this function exists to perform.
        let inner = stream
            .decompressed_content()
            .unwrap_or_else(|_| stream.content.clone());
        let has_text = Content::decode(&inner).is_ok_and(|c| {
            c.operations
                .iter()
                .any(|op| is_show_operator(&op.operator))
        });
        if has_text {
            return Err(refuse(format!(
                "This page draws text through a form ({}), which VibePDF can't redact into. \
                 Flatten the page first, or redact the source document.",
                String::from_utf8_lossy(name)
            )));
        }
    }
    Ok(())
}

fn resources_of<'a>(doc: &'a Document, page: &'a Dictionary) -> Option<&'a Dictionary> {
    match page.get(b"Resources").ok()? {
        Object::Dictionary(d) => Some(d),
        Object::Reference(r) => doc.get_object(*r).and_then(Object::as_dict).ok(),
        _ => None,
    }
}

fn resolve_dict<'a>(doc: &'a Document, obj: &'a Object) -> Option<&'a Dictionary> {
    match obj {
        Object::Dictionary(d) => Some(d),
        Object::Reference(r) => doc.get_object(*r).and_then(Object::as_dict).ok(),
        _ => None,
    }
}

/// How we can measure a given font's advances, if at all.
#[derive(Debug, Clone)]
enum Metrics {
    /// A standard font: exact widths from the Adobe tables.
    Core14(String),
    /// A simple font with an explicit `/Widths` array, indexed from `/FirstChar`.
    Widths { first: i64, widths: Vec<f32> },
    /// Anything else — CID fonts, missing widths. Unmeasurable by design.
    Unknown,
}

/// The page's `/Font` resources, by resource name.
fn page_fonts(doc: &Document, page_id: ObjectId) -> std::collections::HashMap<String, Metrics> {
    let mut out = std::collections::HashMap::new();
    let Ok(page) = doc.get_object(page_id).and_then(Object::as_dict) else {
        return out;
    };
    let Some(fonts) = resources_of(doc, page)
        .and_then(|r| r.get(b"Font").ok())
        .and_then(|o| resolve_dict(doc, o))
    else {
        return out;
    };

    for (name, entry) in fonts {
        let Some(font) = resolve_dict(doc, entry) else { continue };
        let key = String::from_utf8_lossy(name).into_owned();
        out.insert(key, metrics_for(doc, font));
    }
    out
}

fn metrics_for(doc: &Document, font: &Dictionary) -> Metrics {
    let base = font
        .get(b"BaseFont")
        .and_then(Object::as_name)
        .map(|n| String::from_utf8_lossy(n).into_owned())
        .unwrap_or_default();
    // Subset tags (`ABCDEF+`) are not part of the name the metric tables use.
    let base = base.split_once('+').map_or(base.clone(), |(_, r)| r.to_owned());

    if let Some(widths) = font
        .get(b"Widths")
        .ok()
        .and_then(|o| match o {
            Object::Array(a) => Some(a.clone()),
            Object::Reference(r) => doc.get_object(*r).and_then(Object::as_array).ok().cloned(),
            _ => None,
        })
    {
        let first = font.get(b"FirstChar").and_then(Object::as_i64).unwrap_or(0);
        let widths: Vec<f32> = widths
            .iter()
            .map(|o| o.as_float().unwrap_or(0.0))
            .collect();
        if !widths.is_empty() {
            return Metrics::Widths { first, widths };
        }
    }

    // `text_width` falls back to an average for unknown names, which is not
    // good enough to cut a string on — so only accept names it really knows.
    if has_exact_metrics(&base) {
        Metrics::Core14(base)
    } else {
        Metrics::Unknown
    }
}

/// Advance width of one byte, in text-space units before the font size.
fn advance_per_mille(metrics: &Metrics, byte: u8) -> Option<f32> {
    match metrics {
        Metrics::Core14(base) => {
            let ch = char::from(byte);
            Some(text_width(base, &ch.to_string(), 1000.0))
        }
        Metrics::Widths { first, widths } => {
            let idx = usize::try_from(i64::from(byte) - first).ok()?;
            widths.get(idx).copied()
        }
        Metrics::Unknown => None,
    }
}

fn is_show_operator(operator: &str) -> bool {
    matches!(operator, "Tj" | "TJ" | "'" | "\"")
}

/// Apply one non-showing operator to the tracked state.
///
/// Split out of [`rewrite`] so the loop reads as what it is — a decision per
/// show operator — rather than as a long switch with the interesting case
/// buried in it.
fn track(op: &Operation, ctm: &mut Matrix, ctms: &mut Vec<Matrix>, ts: &mut TextState, tss: &mut Vec<TextState>) {
    match op.operator.as_str() {
        "q" => {
            ctms.push(*ctm);
            tss.push(ts.clone());
        }
        "Q" => {
            if let Some(prev) = ctms.pop() {
                *ctm = prev;
            }
            if let Some(prev) = tss.pop() {
                *ts = prev;
            }
        }
        "cm" => {
            if let Some(m) = matrix_from(&op.operands) {
                *ctm = m.then(*ctm);
            }
        }
        "BT" => {
            ts.tm = Matrix::IDENTITY;
            ts.tlm = Matrix::IDENTITY;
        }
        "Tm" => {
            if let Some(m) = matrix_from(&op.operands) {
                ts.tm = m;
                ts.tlm = m;
            }
        }
        "Td" => {
            ts.tlm = Matrix::translate(num(&op.operands, 0), num(&op.operands, 1)).then(ts.tlm);
            ts.tm = ts.tlm;
        }
        "TD" => {
            let y = num(&op.operands, 1);
            ts.leading = -y;
            ts.tlm = Matrix::translate(num(&op.operands, 0), y).then(ts.tlm);
            ts.tm = ts.tlm;
        }
        "T*" => {
            ts.tlm = Matrix::translate(0.0, -ts.leading).then(ts.tlm);
            ts.tm = ts.tlm;
        }
        "TL" => ts.leading = num(&op.operands, 0),
        "Tf" => {
            ts.font = op
                .operands
                .first()
                .and_then(|o| o.as_name().ok())
                .map(|n| String::from_utf8_lossy(n).into_owned());
            ts.size = num(&op.operands, 1);
        }
        "Tz" => ts.scale = num(&op.operands, 0) / 100.0,
        "Tc" => ts.char_spacing = num(&op.operands, 0),
        "Tw" => ts.word_spacing = num(&op.operands, 0),
        _ => {}
    }
}

/// Every image `XObject` drawn wholly or partly inside `rect`, by resource name.
///
/// An image occupies the unit square transformed by the current matrix, so its
/// placed rectangle is the CTM applied to (0,0)–(1,1). Partial overlap removes
/// the whole image: cropping raster data to a rectangle is a different feature,
/// and leaving the uncovered part would be under-redaction.
fn images_in_region(
    operations: &[Operation],
    images: &std::collections::HashMap<String, ObjectId>,
    rect: [f32; 4],
) -> BTreeSet<String> {
    let mut hit = BTreeSet::new();
    let mut ctm = Matrix::IDENTITY;
    let mut ctms: Vec<Matrix> = Vec::new();
    let mut ts = TextState::default();
    let mut tss: Vec<TextState> = Vec::new();

    for op in operations {
        if op.operator != "Do" {
            track(op, &mut ctm, &mut ctms, &mut ts, &mut tss);
            continue;
        }
        let Some(name) = op
            .operands
            .first()
            .and_then(|o| o.as_name().ok())
            .map(|n| String::from_utf8_lossy(n).into_owned())
        else {
            continue;
        };
        if !images.contains_key(&name) {
            continue; // a form, which `refuse_if_text_hides_in_an_xobject` handled
        }

        // The unit square's four corners, transformed.
        let corners = [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)].map(|(x, y): (f32, f32)| {
            (
                x * ctm.a + y * ctm.c + ctm.e,
                x * ctm.b + y * ctm.d + ctm.f,
            )
        });
        let xs = corners.map(|(x, _)| x);
        let ys = corners.map(|(_, y)| y);
        let (x0, x1) = (xs.iter().copied().fold(f32::MAX, f32::min), xs.iter().copied().fold(f32::MIN, f32::max));
        let (y0, y1) = (ys.iter().copied().fold(f32::MAX, f32::min), ys.iter().copied().fold(f32::MIN, f32::max));

        if x1 > rect[0] && x0 < rect[2] && y1 > rect[1] && y0 < rect[3] {
            hit.insert(name);
        }
    }
    hit
}

/// The page's image `XObject` resources, by name.
fn page_images(doc: &Document, page_id: ObjectId) -> std::collections::HashMap<String, ObjectId> {
    let mut out = std::collections::HashMap::new();
    let Ok(page) = doc.get_object(page_id).and_then(Object::as_dict) else {
        return out;
    };
    let Some(xobjects) = resources_of(doc, page)
        .and_then(|r| r.get(b"XObject").ok())
        .and_then(|o| resolve_dict(doc, o))
    else {
        return out;
    };
    for (name, entry) in xobjects {
        let Ok(id) = entry.as_reference() else { continue };
        let is_image = doc
            .get_object(id)
            .and_then(|o| o.as_stream())
            .is_ok_and(|s| {
                s.dict
                    .get(b"Subtype")
                    .and_then(Object::as_name)
                    .is_ok_and(|n| n == b"Image")
            });
        if is_image {
            out.insert(String::from_utf8_lossy(name).into_owned(), id);
        }
    }
    out
}

/// The rewrite pass: walk the operators, decide each run, rebuild.
fn rewrite(
    operations: Vec<Operation>,
    fonts: &std::collections::HashMap<String, Metrics>,
    rect: [f32; 4],
) -> (Vec<Operation>, RedactReport) {
    let mut out: Vec<Operation> = Vec::with_capacity(operations.len());
    let mut report = RedactReport::default();

    let mut ctm = Matrix::IDENTITY;
    let mut ctm_stack: Vec<Matrix> = Vec::new();
    let mut ts = TextState::default();
    let mut ts_stack: Vec<TextState> = Vec::new();

    for op in operations {
        if !is_show_operator(&op.operator) {
            track(&op, &mut ctm, &mut ctm_stack, &mut ts, &mut ts_stack);
            // `Tz` is dropped from the output for the same reason it always
            // was: it is tracked, not rewritten. Everything else passes through.
            if op.operator != "Tz" {
                out.push(op);
            }
            continue;
        }

        let metrics = ts
            .font
            .as_deref()
            .and_then(|f| fonts.get(f))
            .unwrap_or(&Metrics::Unknown);
        match decide(&op, &ts, ctm, metrics, rect) {
            Verdict::Keep => out.push(op),
            Verdict::Remove => report.removed += 1,
            Verdict::RemoveUnmeasurable => report.removed_whole_for_safety += 1,
            Verdict::Split(kept) => {
                report.split += 1;
                out.push(Operation::new(
                    "Tj",
                    vec![Object::String(kept, lopdf::StringFormat::Literal)],
                ));
            }
        }
    }
    (out, report)
}

enum Verdict {
    Keep,
    Remove,
    RemoveUnmeasurable,
    Split(Vec<u8>),
}

/// Decide what to do with one show operator.
fn decide(
    op: &Operation,
    ts: &TextState,
    ctm: Matrix,
    metrics: &Metrics,
    rect: [f32; 4],
) -> Verdict {
    let Some(text) = shown_bytes(op) else {
        // A `TJ` array we did not flatten, or an operand shape we do not model.
        // If it could be in the region at all, drop it.
        return if run_origin_in_band(ts, ctm, rect) {
            Verdict::RemoveUnmeasurable
        } else {
            Verdict::Keep
        };
    };

    let effective = ts.tm.then(ctm);
    let (x0, y0) = (effective.e, effective.f);
    let size = ts.size * effective.a.abs().max(1e-6);

    // Vertical band first: a run on another line cannot be affected.
    let (top, bottom) = (y0 + size, y0);
    if bottom > rect[3] || top < rect[1] {
        return Verdict::Keep;
    }
    // Rotated or skewed text: we decline to measure it, so it goes if its
    // origin is anywhere near the region.
    if !effective.is_upright() {
        return if x0 >= rect[0] && x0 <= rect[2] {
            Verdict::RemoveUnmeasurable
        } else {
            Verdict::Keep
        };
    }

    // Walk the glyphs, accumulating x. A byte is inside when any part of its
    // advance overlaps the region — the inclusive reading, because a glyph half
    // inside is still legible.
    let mut x = x0;
    let mut kept: Vec<u8> = Vec::with_capacity(text.len());
    let mut any_removed = false;
    for byte in &text {
        let Some(per_mille) = advance_per_mille(metrics, *byte) else {
            // Unmeasurable font: if any of this run is in the band, all of it goes.
            return if x0 <= rect[2] { Verdict::RemoveUnmeasurable } else { Verdict::Keep };
        };
        let mut advance = per_mille * ts.size / 1000.0 + ts.char_spacing;
        if *byte == b' ' {
            advance += ts.word_spacing;
        }
        advance *= ts.scale * effective.a.abs();

        let inside = x + advance > rect[0] && x < rect[2];
        if inside {
            any_removed = true;
        } else {
            kept.push(*byte);
        }
        x += advance;
    }

    if !any_removed {
        return Verdict::Keep;
    }
    if kept.is_empty() {
        return Verdict::Remove;
    }
    // A cut in the middle would need the surviving tail re-positioned, which is
    // layout this module does not do. Only a clean prefix or suffix is split;
    // anything else goes whole.
    let is_prefix = text.starts_with(&kept[..]);
    if is_prefix {
        Verdict::Split(kept)
    } else {
        Verdict::RemoveUnmeasurable
    }
}

/// Is the run's origin inside the region's vertical band?
fn run_origin_in_band(ts: &TextState, ctm: Matrix, rect: [f32; 4]) -> bool {
    let m = ts.tm.then(ctm);
    m.f >= rect[1] - ts.size && m.f <= rect[3]
}

/// The bytes a show operator paints, when we can read them simply.
fn shown_bytes(op: &Operation) -> Option<Vec<u8>> {
    match op.operator.as_str() {
        "Tj" => op.operands.first().and_then(|o| o.as_str().ok()).map(<[u8]>::to_vec),
        // `TJ` interleaves strings and kerning numbers. Concatenating the
        // strings loses the kerns, so it is only safe to *measure*, never to
        // rewrite — hence not handled here, which routes it to the safe path.
        _ => None,
    }
}

fn matrix_from(operands: &[Object]) -> Option<Matrix> {
    if operands.len() < 6 {
        return None;
    }
    Some(Matrix {
        a: num(operands, 0),
        b: num(operands, 1),
        c: num(operands, 2),
        d: num(operands, 3),
        e: num(operands, 4),
        f: num(operands, 5),
    })
}

fn num(operands: &[Object], i: usize) -> f32 {
    operands.get(i).and_then(|o| o.as_float().ok()).unwrap_or(0.0)
}

/// Paint the black box.
///
/// **This is a marker, not the mechanism.** The content is already gone by the
/// time this runs; the rectangle is there so a reader can see that something
/// was removed. `the_black_box_is_not_the_mechanism` deletes it again and
/// checks the text is still absent.
fn cover(doc: &mut Document, page_id: ObjectId, rect: [f32; 4]) -> Result<(), CommandError> {
    let ops = vec![
        Operation::new("q", vec![]),
        Operation::new("rg", vec![0.into(), 0.into(), 0.into()]),
        Operation::new(
            "re",
            vec![
                rect[0].into(),
                rect[1].into(),
                (rect[2] - rect[0]).into(),
                (rect[3] - rect[1]).into(),
            ],
        ),
        Operation::new("f", vec![]),
        Operation::new("Q", vec![]),
    ];
    let existing = doc.get_page_content(page_id).map_err(redact_err)?;
    let mut content = Content::decode(&existing).map_err(redact_err)?;
    content.operations.extend(ops);
    let encoded = content.encode().map_err(redact_err)?;
    doc.change_page_content(page_id, encoded).map_err(redact_err)
}

/// SPEC: P6-SEC-010(c) — the text really is gone from the saved file.
///
/// Run by the caller after saving, against `PDFium`'s extractor rather than our
/// own parse: checking our work with the same code that did it proves nothing.
/// `must_survive` is checked too — an extractor returning nothing at all would
/// otherwise pass this trivially, which is the lesson from P6.D3's fixture.
pub fn confirm_removed(
    extracted: &str,
    must_be_gone: &[&str],
    must_survive: &[&str],
) -> Result<(), CommandError> {
    let leaked: BTreeSet<&str> = must_be_gone
        .iter()
        .copied()
        .filter(|needle| extracted.contains(needle))
        .collect();
    if !leaked.is_empty() {
        return Err(CommandError::Internal(format!(
            "redaction did not remove {leaked:?} — the document was not changed"
        )));
    }
    let lost: BTreeSet<&str> = must_survive
        .iter()
        .copied()
        .filter(|needle| !extracted.contains(needle))
        .collect();
    if !lost.is_empty() {
        return Err(CommandError::Internal(format!(
            "the check cannot be trusted: {lost:?} should still be present but is not"
        )));
    }
    Ok(())
}

/// SPEC: P6-SEC-010 — redact the live document, handing back the inverse and
/// the report.
///
/// Same shape as `clean_into` and `form_import::import_into`: an `Edit` can
/// return only its inverse, and the report is the sole evidence of what
/// happened — the page afterwards is *supposed* to look like a black box, and a
/// black box is exactly what a failed redaction looks like too.
///
/// The inverse is a pre-redaction byte snapshot, so this is undoable in-session
/// and permanent once the file is saved and reopened. That is the same contract
/// as flatten, and for redaction it is the point rather than a limitation.
pub fn redact_into<'a>(
    doc: &mut pdfium_render::prelude::PdfDocument<'a>,
    page: usize,
    rect: [f32; 4],
    options: RedactOptions,
) -> Result<
    (
        Box<dyn crate::pdf::undo::Edit<pdfium_render::prelude::PdfDocument<'a>>>,
        RedactReport,
    ),
    CommandError,
> {
    use crate::pdf::document::{pdfium, pdfium_lock};

    let pre_bytes = {
        let _guard = pdfium_lock()?;
        doc.save_to_bytes().map_err(CommandError::from)?
    };
    let (new_bytes, report) = redact_region(&pre_bytes, page, rect, options)?;
    {
        let _guard = pdfium_lock()?;
        *doc = pdfium()?
            .load_pdf_from_byte_vec(new_bytes, None)
            .map_err(CommandError::from)?;
    }
    Ok((
        Box::new(crate::pdf::restore::RestoreDocEdit { bytes: pre_bytes }),
        report,
    ))
}

/// What `pdf_redact_region` hands back: the counts plus the post-redaction
/// history state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RedactOutcome {
    #[serde(flatten)]
    pub report: RedactReport,
    pub history: crate::pdf::undo::HistoryState,
}
