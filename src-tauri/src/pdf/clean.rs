//! Clean a document of everything that is not its visible content (P6.D3).
//!
//! SPEC: P6-SEC-012 — "WHEN the user invokes 'Clean document,' THE system SHALL
//! remove: metadata (author, creator, producer, custom keys), hidden text,
//! comments, attachments, bookmarks, form data, embedded files — each
//! toggle-able."
//!
//! A COS (lopdf) byte→byte transform like [`crate::pdf::flatten`], wrapped by
//! the caller in a `cos_edit` whose inverse is a pre-clean snapshot: undoable
//! in-session, gone for good once the file is saved and reopened.
//!
//! **The thing to get right is that removal removes.** Detaching a key from a
//! dictionary leaves the object it pointed at sitting in the file, where any
//! text search still finds it; a cleaner that does only that reports success
//! and ships the author's name anyway. Every removal here therefore deletes the
//! *object*, and the tests assert on the marker strings' absence from the saved
//! bytes rather than on the keys we happened to unset.
//!
//! The other trap is that a PDF stores its metadata **twice**: the classic
//! `/Info` dictionary and an XMP packet in the catalog's `/Metadata` stream.
//! They routinely disagree, readers differ on which they believe, and clearing
//! only `/Info` is the single most common way a "cleaned" PDF still names its
//! author. Both go, along with any page-level `/Metadata`.

use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object, ObjectId};
use pdfium_render::prelude::PdfDocument;

use crate::error::CommandError;
use crate::pdf::document::{pdfium, pdfium_lock};
use crate::pdf::restore::RestoreDocEdit;
use crate::pdf::undo::Edit;

#[allow(clippy::needless_pass_by_value)]
fn clean_err(e: lopdf::Error) -> CommandError {
    CommandError::PdfError(format!("clean: {e}"))
}

/// SPEC: P6-SEC-012 — which of the seven categories to remove.
///
/// Every field is "remove this", so [`Default`] — all `false` — is the document
/// untouched. That is the safe direction here, and the opposite of
/// `DocumentPermissions`, where the derived default would have been the most
/// restrictive option rather than the least destructive one.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanOptions {
    /// `/Info`, the XMP `/Metadata` packet, and page-level `/Metadata`.
    pub metadata: bool,
    /// Text drawn in an invisible rendering mode (`3 Tr`, `7 Tr`).
    pub hidden_text: bool,
    /// Markup annotations — notes, highlights, ink, stamps. Not links, not form
    /// widgets, and not file attachments (those are their own toggle).
    pub comments: bool,
    /// `/FileAttachment` annotations.
    pub attachments: bool,
    /// The `/Outlines` tree.
    pub bookmarks: bool,
    /// Field *values*, leaving the empty form behind.
    pub form_data: bool,
    /// The document-level `/Names /EmbeddedFiles` tree.
    pub embedded_files: bool,
}

/// What a clean actually removed, per category.
///
/// Reported back to the user because "Clean document" otherwise gives no sign
/// of having done anything — the visible page is unchanged by design, so the
/// counts are the only feedback that distinguishes a working clean from a
/// silent no-op.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanReport {
    /// Entries removed from `/Info`, including custom keys.
    pub info_keys: usize,
    /// XMP packets removed, from the catalog and from pages.
    pub xmp_packets: usize,
    /// Text-showing operators dropped for being invisible.
    pub hidden_text_runs: usize,
    pub comments: usize,
    pub attachments: usize,
    pub bookmarks: usize,
    /// Fields whose value was cleared.
    pub form_fields: usize,
    pub embedded_files: usize,
}

impl CleanReport {
    /// Whether anything at all was removed. Drives the "nothing to clean"
    /// message rather than a success toast for a no-op.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// SPEC: P6-SEC-012 — return `bytes` with everything `opts` asks for removed.
pub fn clean_document(
    bytes: &[u8],
    opts: &CleanOptions,
) -> Result<(Vec<u8>, CleanReport), CommandError> {
    let mut doc = Document::load_mem(bytes).map_err(clean_err)?;
    let mut report = CleanReport::default();

    // Order matters in one place: attachments before embedded files, so the
    // annotation is gone before the name tree that shares its file stream.
    if opts.metadata {
        strip_metadata(&mut doc, &mut report)?;
    }
    if opts.bookmarks {
        strip_bookmarks(&mut doc, &mut report)?;
    }
    if opts.attachments {
        report.attachments = strip_annotations(&mut doc, |s| s == b"FileAttachment")?;
    }
    if opts.comments {
        report.comments = strip_annotations(&mut doc, is_markup_subtype)?;
    }
    if opts.embedded_files {
        strip_embedded_files(&mut doc, &mut report)?;
    }
    if opts.form_data {
        report.form_fields = strip_form_values(&mut doc)?;
    }
    if opts.hidden_text {
        report.hidden_text_runs = strip_hidden_text(&mut doc)?;
    }

    let mut out = Vec::new();
    doc.save_to(&mut out)
        .map_err(|e| CommandError::PdfError(format!("clean: lopdf save: {e}")))?;
    Ok((out, report))
}

/// The catalog's dictionary, by id so callers can write back through it.
fn catalog_id(doc: &Document) -> Result<ObjectId, CommandError> {
    doc.trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .map_err(|e| CommandError::PdfError(format!("clean: no /Root: {e}")))
}

/// Detach a reference *and* delete what it pointed at.
///
/// The whole module hinges on this pair. `dict.remove(key)` alone leaves an
/// orphan object in the saved file with its contents intact and searchable —
/// which is indistinguishable from a successful clean unless you look at the
/// bytes.
fn drop_entry(doc: &mut Document, holder: ObjectId, key: &[u8]) -> Result<bool, CommandError> {
    let target = {
        let dict = doc
            .get_object_mut(holder)
            .and_then(Object::as_dict_mut)
            .map_err(clean_err)?;
        let Some(obj) = dict.remove(key) else {
            return Ok(false);
        };
        obj.as_reference().ok()
    };
    if let Some(id) = target {
        doc.objects.remove(&id);
    }
    Ok(true)
}

/// `/Info` (every key, including custom ones) plus every XMP packet.
fn strip_metadata(doc: &mut Document, report: &mut CleanReport) -> Result<(), CommandError> {
    if let Ok(id) = doc.trailer.get(b"Info").and_then(Object::as_reference) {
        report.info_keys = doc
            .get_object(id)
            .and_then(Object::as_dict)
            .map_or(0, Dictionary::len);
        doc.objects.remove(&id);
    }
    doc.trailer.remove(b"Info");

    let root = catalog_id(doc)?;
    if drop_entry(doc, root, b"Metadata")? {
        report.xmp_packets += 1;
    }
    // Pages may carry their own XMP. Rare, and exactly the sort of leftover a
    // catalog-only sweep misses.
    for (_, page_id) in doc.get_pages() {
        if drop_entry(doc, page_id, b"Metadata")? {
            report.xmp_packets += 1;
        }
    }
    Ok(())
}

/// The `/Outlines` tree, counted by the items actually deleted.
fn strip_bookmarks(doc: &mut Document, report: &mut CleanReport) -> Result<(), CommandError> {
    let root = catalog_id(doc)?;
    let outlines = {
        let dict = doc
            .get_object_mut(root)
            .and_then(Object::as_dict_mut)
            .map_err(clean_err)?;
        dict.remove(b"Outlines").and_then(|o| o.as_reference().ok())
    };
    let Some(outlines) = outlines else {
        return Ok(());
    };

    // Walk /First … /Next through every level rather than trusting /Count,
    // which producers are free to get wrong and which counts open items only.
    let mut stack = vec![outlines];
    let mut seen = Vec::new();
    while let Some(id) = stack.pop() {
        if seen.contains(&id) {
            continue; // outline trees are doubly linked; do not loop forever
        }
        seen.push(id);
        if let Ok(dict) = doc.get_object(id).and_then(Object::as_dict) {
            for key in [b"First".as_slice(), b"Next".as_slice(), b"Last".as_slice()] {
                if let Ok(next) = dict.get(key).and_then(Object::as_reference) {
                    stack.push(next);
                }
            }
        }
    }
    for id in &seen {
        doc.objects.remove(id);
    }
    // The root itself is not a bookmark.
    report.bookmarks = seen.len().saturating_sub(1);
    Ok(())
}

/// Markup annotations, per PDF 32000-1 Table 169 — everything a person added as
/// a comment. `/Link` and `/Widget` are page furniture rather than commentary,
/// and `/FileAttachment` has its own toggle.
fn is_markup_subtype(subtype: &[u8]) -> bool {
    matches!(
        subtype,
        b"Text"
            | b"FreeText"
            | b"Line"
            | b"Square"
            | b"Circle"
            | b"Polygon"
            | b"PolyLine"
            | b"Highlight"
            | b"Underline"
            | b"Squiggly"
            | b"StrikeOut"
            | b"Stamp"
            | b"Caret"
            | b"Ink"
            | b"Popup"
            | b"Sound"
            | b"Movie"
            | b"Redact"
    )
}

/// Remove every annotation whose `/Subtype` matches, from every page.
fn strip_annotations(
    doc: &mut Document,
    matches: impl Fn(&[u8]) -> bool,
) -> Result<usize, CommandError> {
    let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
    let mut removed = 0;

    for page_id in pages {
        let annots = match doc.get_object(page_id).and_then(Object::as_dict) {
            Ok(d) => match d.get(b"Annots") {
                Ok(Object::Array(a)) => a.clone(),
                Ok(Object::Reference(r)) => match doc.get_object(*r).and_then(Object::as_array) {
                    Ok(a) => a.clone(),
                    Err(_) => continue,
                },
                _ => continue,
            },
            Err(_) => continue,
        };

        let mut keep = Vec::with_capacity(annots.len());
        let mut doomed = Vec::new();
        for entry in annots {
            let id = entry.as_reference().ok();
            let subtype = id
                .and_then(|i| doc.get_object(i).ok())
                .and_then(|o| o.as_dict().ok())
                .and_then(|d| d.get(b"Subtype").ok())
                .and_then(|o| o.as_name().ok())
                .map(<[u8]>::to_vec);

            match (subtype, id) {
                (Some(s), Some(i)) if matches(&s) => {
                    doomed.push(i);
                    removed += 1;
                }
                _ => keep.push(entry),
            }
        }
        if doomed.is_empty() {
            continue;
        }

        // A markup annotation's /Popup is a separate object that the page may
        // not list. Left behind it becomes an orphan carrying the same text.
        let mut popups = Vec::new();
        for id in &doomed {
            if let Ok(d) = doc.get_object(*id).and_then(Object::as_dict) {
                if let Ok(p) = d.get(b"Popup").and_then(Object::as_reference) {
                    popups.push(p);
                }
            }
        }
        for id in doomed.into_iter().chain(popups) {
            doc.objects.remove(&id);
        }

        let dict = doc
            .get_object_mut(page_id)
            .and_then(Object::as_dict_mut)
            .map_err(clean_err)?;
        dict.set("Annots", Object::Array(keep));
    }
    Ok(removed)
}

/// The document-level `/Names /EmbeddedFiles` tree, and the file streams it
/// names. Counted in files, not tree nodes.
fn strip_embedded_files(doc: &mut Document, report: &mut CleanReport) -> Result<(), CommandError> {
    let root = catalog_id(doc)?;
    let names_id = doc
        .get_object(root)
        .and_then(Object::as_dict)
        .ok()
        .and_then(|d| d.get(b"Names").ok())
        .and_then(|o| o.as_reference().ok());

    // /Names may be a direct dictionary or a reference; handle both.
    let mut tree: Option<Dictionary> = doc
        .get_object(root)
        .and_then(Object::as_dict)
        .ok()
        .and_then(|d| d.get(b"Names").ok())
        .and_then(|o| o.as_dict().ok())
        .cloned();
    if tree.is_none() {
        if let Some(id) = names_id {
            tree = doc.get_object(id).and_then(Object::as_dict).ok().cloned();
        }
    }
    let Some(tree) = tree else { return Ok(()) };

    let ef = tree
        .get(b"EmbeddedFiles")
        .ok()
        .and_then(|o| match o {
            Object::Dictionary(d) => Some(d.clone()),
            Object::Reference(r) => doc.get_object(*r).and_then(Object::as_dict).ok().cloned(),
            _ => None,
        })
        .map(|d| (d.get(b"Names").ok().cloned(), d));
    let Some((names, _)) = ef else { return Ok(()) };

    // /Names is [ (name) filespec (name) filespec … ]; the filespecs are ours
    // to delete, along with the streams their /EF points at.
    if let Some(Object::Array(entries)) = names {
        for spec in entries.iter().filter_map(|o| o.as_reference().ok()) {
            if let Ok(d) = doc.get_object(spec).and_then(Object::as_dict) {
                let streams: Vec<ObjectId> = d
                    .get(b"EF")
                    .and_then(Object::as_dict)
                    .map(|ef| ef.iter().filter_map(|(_, v)| v.as_reference().ok()).collect())
                    .unwrap_or_default();
                for s in streams {
                    doc.objects.remove(&s);
                }
            }
            doc.objects.remove(&spec);
            report.embedded_files += 1;
        }
    }

    // Drop the subtree, and /Names itself if that is all it held.
    let names_dict_id = names_id;
    if let Some(id) = names_dict_id {
        drop_entry(doc, id, b"EmbeddedFiles")?;
        let empty = doc
            .get_object(id)
            .and_then(Object::as_dict)
            .is_ok_and(Dictionary::is_empty);
        if empty {
            drop_entry(doc, root, b"Names")?;
        }
    } else {
        let dict = doc
            .get_object_mut(root)
            .and_then(Object::as_dict_mut)
            .map_err(clean_err)?;
        if let Ok(Object::Dictionary(inner)) = dict.get_mut(b"Names") {
            inner.remove(b"EmbeddedFiles");
            let empty = inner.is_empty();
            if empty {
                dict.remove(b"Names");
            }
        }
    }
    Ok(())
}

/// Clear every field's value, leaving the form itself in place.
///
/// The spec says "form data", not "the form": a cleaned document should still
/// be fillable. `/AP` goes with the value because it is a picture *of* the
/// value — a field whose `/V` is gone but whose appearance still reads
/// "SECRETFORMVALUE" has not been cleaned in any sense the user means.
fn strip_form_values(doc: &mut Document) -> Result<usize, CommandError> {
    let root = catalog_id(doc)?;
    let acro = doc
        .get_object(root)
        .and_then(Object::as_dict)
        .ok()
        .and_then(|d| d.get(b"AcroForm").ok())
        .and_then(|o| match o {
            Object::Reference(r) => Some(*r),
            _ => None,
        });
    let Some(acro) = acro else { return Ok(0) };

    let mut stack: Vec<ObjectId> = doc
        .get_object(acro)
        .and_then(Object::as_dict)
        .ok()
        .and_then(|d| d.get(b"Fields").ok())
        .and_then(|o| o.as_array().ok())
        .map(|a| a.iter().filter_map(|o| o.as_reference().ok()).collect())
        .unwrap_or_default();

    let mut cleared = 0;
    let mut seen: Vec<ObjectId> = Vec::new();
    while let Some(id) = stack.pop() {
        if seen.contains(&id) {
            continue;
        }
        seen.push(id);

        let kids: Vec<ObjectId> = doc
            .get_object(id)
            .and_then(Object::as_dict)
            .ok()
            .and_then(|d| d.get(b"Kids").ok())
            .and_then(|o| o.as_array().ok())
            .map(|a| a.iter().filter_map(|o| o.as_reference().ok()).collect())
            .unwrap_or_default();
        stack.extend(kids);

        let Ok(dict) = doc.get_object_mut(id).and_then(Object::as_dict_mut) else {
            continue;
        };
        let had_value = dict.has(b"V") || dict.has(b"DV");
        dict.remove(b"V");
        dict.remove(b"DV");
        // Checkboxes and radios show state through /AS; leaving it set shows the
        // old choice even with no value behind it.
        dict.remove(b"AS");
        dict.remove(b"AP");
        if had_value {
            cleared += 1;
        }
    }

    // Readers rebuild the (now empty) appearances on open.
    if let Ok(dict) = doc.get_object_mut(acro).and_then(Object::as_dict_mut) {
        dict.set("NeedAppearances", Object::Boolean(true));
    }
    Ok(cleared)
}

/// Text painted in an invisible rendering mode.
///
/// Mode 3 is "neither fill nor stroke" and mode 7 is "clip only"; both put
/// glyphs in the file that no reader draws and every text extractor finds.
///
/// **This is also how a scanned page is made searchable.** OCR puts its
/// recognised text under the page image in mode 3, so removing hidden text
/// un-searches a scan — and P7 will produce exactly such layers. The toggle is
/// off by default and the dialog says so; this is a real cost, not a caveat.
fn strip_hidden_text(doc: &mut Document) -> Result<usize, CommandError> {
    let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
    let mut removed = 0;

    for page_id in pages {
        // No content stream on this page; nothing to strip.
        let Ok(data) = doc.get_page_content(page_id) else {
            continue;
        };
        let Ok(content) = Content::decode(&data) else {
            continue; // unparseable content is left exactly as it was
        };

        let mut out: Vec<Operation> = Vec::with_capacity(content.operations.len());
        // Rendering mode is graphics state, so `q`/`Q` save and restore it and
        // it survives BT/ET. Tracking it any other way mis-attributes runs.
        let mut saved: Vec<i64> = Vec::new();
        let mut mode: i64 = 0;
        let mut page_removed = 0;

        for op in content.operations {
            match op.operator.as_str() {
                "q" => {
                    saved.push(mode);
                    out.push(op);
                }
                "Q" => {
                    if let Some(prev) = saved.pop() {
                        mode = prev;
                    }
                    out.push(op);
                }
                "Tr" => {
                    mode = op.operands.first().and_then(|o| o.as_i64().ok()).unwrap_or(0);
                    out.push(op);
                }
                "Tj" | "TJ" if mode == 3 || mode == 7 => page_removed += 1,
                // `'` and `"` move to the next line *and* show. Dropping them
                // whole would shift every following run up a line, so keep the
                // movement and discard only the painting.
                "'" if mode == 3 || mode == 7 => {
                    page_removed += 1;
                    out.push(Operation::new("T*", vec![]));
                }
                "\"" if mode == 3 || mode == 7 => {
                    page_removed += 1;
                    let mut ops = op.operands;
                    if ops.len() >= 2 {
                        out.push(Operation::new("Tw", vec![ops.remove(0)]));
                        out.push(Operation::new("Tc", vec![ops.remove(0)]));
                    }
                    out.push(Operation::new("T*", vec![]));
                }
                _ => out.push(op),
            }
        }

        if page_removed == 0 {
            continue;
        }
        let encoded = Content { operations: out }.encode().map_err(clean_err)?;
        doc.change_page_content(page_id, encoded)
            .map_err(clean_err)?;
        removed += page_removed;
    }
    Ok(removed)
}

/// SPEC: P6-SEC-012 — clean the live document and hand back both the inverse
/// and the report.
///
/// Same shape as [`crate::pdf::form_import::import_into`]: an `Edit` alone
/// cannot carry a report out (its return value is the inverse), and the counts
/// are the only feedback a clean produces — the page looks identical either
/// way. The inverse is a pre-clean byte snapshot, so this is undoable in-session
/// and permanent once the file is saved and reopened.
pub fn clean_into<'a>(
    doc: &mut PdfDocument<'a>,
    opts: &CleanOptions,
) -> Result<(Box<dyn Edit<PdfDocument<'a>>>, CleanReport), CommandError> {
    let pre_bytes = {
        let _guard = pdfium_lock()?;
        doc.save_to_bytes().map_err(CommandError::from)?
    };
    let (new_bytes, report) = clean_document(&pre_bytes, opts)?;
    {
        let _guard = pdfium_lock()?;
        *doc = pdfium()?
            .load_pdf_from_byte_vec(new_bytes, None)
            .map_err(CommandError::from)?;
    }
    Ok((Box::new(RestoreDocEdit { bytes: pre_bytes }), report))
}

/// What `pdf_clean_document` hands back: the per-category counts plus the
/// post-clean history state, so the frontend updates Undo/Redo in the same
/// round-trip every other write command uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanOutcome {
    #[serde(flatten)]
    pub report: CleanReport,
    pub history: crate::pdf::undo::HistoryState,
}
