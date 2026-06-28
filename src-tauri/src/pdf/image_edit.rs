//! Image editing (P4.C2) — move / resize / rotate / delete an existing image.
//!
//! SPEC: P4-EDIT-006. Two write paths, mirroring the text engine:
//! - **Transform** ([`transform_image`]) overrides an image's placement matrix via
//!   `PDFium`'s `reset_matrix` — a *mutate-in-place* FFI like `FPDFText_SetText`
//!   (which works), not `FPDFPage_RemoveObject` (which crashes). Move, resize and
//!   rotate are all just a new matrix the frontend computes, so they share this one
//!   primitive. Runs on a **throwaway** doc (never the live one) + `Manual`
//!   regeneration, exactly like [`crate::pdf::reflow::replace_text_run`].
//! - **Delete** ([`delete_image`]) removes the image's `Do` at the lopdf level (the
//!   B3 splice pattern), verified by re-extraction — because `remove_object`
//!   SIGSEGVs.

use std::collections::HashSet;

use lopdf::content::Content;
use lopdf::{Document, Object};
use pdfium_render::prelude::*;

use crate::error::CommandError;
use crate::pdf::document::{pdfium, pdfium_lock};
use crate::pdf::image_extract::extract_images_from_bytes;
use crate::pdf::image_xobject::embed_image;
use crate::pdf::restore::RestoreDocEdit;
use crate::pdf::undo::Edit;

#[allow(clippy::needless_pass_by_value)]
fn lopdf_err(e: lopdf::Error) -> CommandError {
    CommandError::PdfError(format!("lopdf: {e}"))
}

/// SPEC: P4-EDIT-006 — override image `index`'s placement matrix to `matrix`
/// (`[a,b,c,d,e,f]`). Move/resize/rotate all funnel here. Returns new document
/// bytes; never mutates the input.
#[allow(clippy::many_single_char_names)] // a,b,c,d,e,f are the PDF matrix components
pub fn transform_image(
    bytes: &[u8],
    page: usize,
    index: usize,
    matrix: [f32; 6],
) -> Result<Vec<u8>, CommandError> {
    let page_index = i32::try_from(page)
        .map_err(|_| CommandError::InvalidInput(format!("bad page index: {page}")))?;

    let _guard = pdfium_lock()?;
    let doc = pdfium()?
        .load_pdf_from_byte_vec(bytes.to_vec(), None)
        .map_err(CommandError::from)?;

    {
        let mut pdf_page = doc.pages().get(page_index).map_err(CommandError::from)?;
        pdf_page.set_content_regeneration_strategy(PdfPageContentRegenerationStrategy::Manual);
        let obj_index = nth_image_object_index(&pdf_page, index)?;

        let mut object = pdf_page.objects().get(obj_index).map_err(CommandError::from)?;
        let PdfPageObject::Image(_) = &object else {
            return Err(CommandError::Internal("located object is not an image".into()));
        };
        let [a, b, c, d, e, f] = matrix;
        object
            .reset_matrix(PdfMatrix::new(a, b, c, d, e, f))
            .map_err(CommandError::from)?;

        pdf_page.regenerate_content().map_err(CommandError::from)?;
    }

    let out = doc.save_to_bytes().map_err(CommandError::from)?;
    drop(doc);
    Ok(out)
}

/// Find the container index of the `image_index`-th **image** object on the page,
/// counting in the same order [`crate::pdf::image_extract::extract_images`] does.
fn nth_image_object_index(page: &PdfPage, image_index: usize) -> Result<usize, CommandError> {
    let mut seen = 0usize;
    for (container_index, object) in page.objects().iter().enumerate() {
        if matches!(object, PdfPageObject::Image(_)) {
            if seen == image_index {
                return Ok(container_index);
            }
            seen += 1;
        }
    }
    Err(CommandError::InvalidInput(format!(
        "image index {image_index} out of range ({seen} images on page)"
    )))
}

/// SPEC: P4-EDIT-006 / P4-EDIT-004 — remove image `index` from the page content
/// stream by splicing out its `Do` operator (the B3 lopdf approach — `PDFium`'s
/// `remove_object` SIGSEGVs). Verified by re-extraction: one fewer image, else
/// error (no silent mis-delete).
pub fn delete_image(bytes: &[u8], page: usize, index: usize) -> Result<Vec<u8>, CommandError> {
    let before = extract_images_from_bytes(bytes, page)?;
    if index >= before.len() {
        return Err(CommandError::InvalidInput(format!(
            "image index {index} out of range ({} images on page)",
            before.len()
        )));
    }

    let new_bytes = splice_out_image_do(bytes, page, index)?;

    // Verify: the surviving images are exactly `before` minus the target, with their
    // geometry unchanged. A mismatch (count *or* a shifted bbox) means the splice
    // disturbed more than the one `Do` — reject rather than corrupt.
    let after = extract_images_from_bytes(&new_bytes, page)?;
    let expected: Vec<[f32; 4]> = before
        .iter()
        .enumerate()
        .filter_map(|(i, img)| (i != index).then_some(img.bbox))
        .collect();
    let matches = after.len() == expected.len()
        && after
            .iter()
            .zip(&expected)
            .all(|(img, exp)| img.bbox.iter().zip(exp).all(|(a, b)| (a - b).abs() < 1.0));
    if !matches {
        return Err(CommandError::PdfError(
            "image deletion did not match the selected image — its draw may live in an \
             XObject form or a content structure we don't rewrite"
                .to_owned(),
        ));
    }
    Ok(new_bytes)
}

/// Decode the page content, drop the `index`-th image-painting `Do` operator,
/// re-encode. Pure lopdf. An image `Do` is one whose `/XObject` resource has
/// `/Subtype /Image` (a Form `XObject`'s `Do` is left alone).
fn splice_out_image_do(bytes: &[u8], page: usize, index: usize) -> Result<Vec<u8>, CommandError> {
    let mut doc = Document::load_mem(bytes).map_err(lopdf_err)?;
    let page_no = u32::try_from(page)
        .ok()
        .map(|n| n + 1)
        .ok_or_else(|| CommandError::InvalidInput(format!("bad page index: {page}")))?;
    let page_id = *doc
        .get_pages()
        .get(&page_no)
        .ok_or_else(|| CommandError::InvalidInput(format!("page index out of range: {page}")))?;

    let image_names = image_xobject_names(&doc, page_id);
    let mut operations = doc.get_and_decode_page_content(page_id).map_err(lopdf_err)?.operations;

    let mut seen = 0usize;
    let mut target = None;
    for (i, op) in operations.iter().enumerate() {
        if op.operator == "Do" {
            if let Some(Object::Name(name)) = op.operands.first() {
                if image_names.contains(name) {
                    if seen == index {
                        target = Some(i);
                        break;
                    }
                    seen += 1;
                }
            }
        }
    }
    let idx = target.ok_or_else(|| {
        CommandError::InvalidInput(format!(
            "image index {index} out of range ({seen} image Do operators in page content)"
        ))
    })?;

    operations.remove(idx);
    let new_content = Content { operations }.encode().map_err(lopdf_err)?;
    doc.change_page_content(page_id, new_content).map_err(lopdf_err)?;

    let mut out = Vec::new();
    doc.save_to(&mut out)
        .map_err(|e| CommandError::PdfError(format!("lopdf save: {e}")))?;
    Ok(out)
}

/// The page's `/Resources /XObject` names whose object has `/Subtype /Image`.
fn image_xobject_names(doc: &Document, page_id: lopdf::ObjectId) -> HashSet<Vec<u8>> {
    let mut names = HashSet::new();
    // Resources may be a direct dict or a reference; XObject likewise.
    let resources = doc
        .get_dictionary(page_id)
        .ok()
        .and_then(|p| p.get(b"Resources").ok())
        .and_then(|o| deref_dict(doc, o));
    let Some(resources) = resources else { return names };
    let Some(xobjects) = resources.get(b"XObject").ok().and_then(|o| deref_dict(doc, o)) else {
        return names;
    };
    for (name, obj) in xobjects {
        let is_image = obj
            .as_reference()
            .ok()
            .and_then(|id| doc.get_object(id).ok())
            .and_then(|o| o.as_stream().ok())
            .and_then(|s| s.dict.get(b"Subtype").ok())
            .and_then(|o| o.as_name().ok())
            == Some(&b"Image"[..]);
        if is_image {
            names.insert(name.clone());
        }
    }
    names
}

/// Resolve an object that may be a direct dictionary or a reference to one.
fn deref_dict<'a>(doc: &'a Document, obj: &'a Object) -> Option<&'a lopdf::Dictionary> {
    match obj {
        Object::Dictionary(d) => Some(d),
        Object::Reference(id) => doc.get_dictionary(*id).ok(),
        _ => None,
    }
}

/// SPEC: P4-EDIT-006 — replace image `index`'s pixel data with `new_image` (PNG or
/// JPEG), **preserving its placement**. Embeds the new image and overwrites the
/// `XObject` the selected image references *in place* (its resource name, `cm`, and
/// `Do` are all untouched — only the pixels change, so no `/Resources` edit and no
/// copy-on-write are needed). Verified by re-extraction: same image count + every
/// bbox unchanged, else error.
pub fn replace_image(
    bytes: &[u8],
    page: usize,
    index: usize,
    new_image: &[u8],
) -> Result<Vec<u8>, CommandError> {
    let before = extract_images_from_bytes(bytes, page)?;
    if index >= before.len() {
        return Err(CommandError::InvalidInput(format!(
            "image index {index} out of range ({} images on page)",
            before.len()
        )));
    }

    let mut doc = Document::load_mem(bytes).map_err(lopdf_err)?;
    let page_no = u32::try_from(page)
        .ok()
        .map(|n| n + 1)
        .ok_or_else(|| CommandError::InvalidInput(format!("bad page index: {page}")))?;
    let page_id = *doc
        .get_pages()
        .get(&page_no)
        .ok_or_else(|| CommandError::InvalidInput(format!("page index out of range: {page}")))?;

    let old_id = nth_image_xobject_id(&doc, page_id, index).ok_or_else(|| {
        CommandError::InvalidInput("could not locate the image's XObject to replace".to_owned())
    })?;

    // Embed the new image, then overwrite the old XObject in place — the resource
    // name still points at `old_id`, which now holds the new pixels (and `/SMask`).
    let new = embed_image(&mut doc, new_image)?;
    let new_object = doc.get_object(new.id).map_err(lopdf_err)?.clone();
    doc.objects.insert(old_id, new_object);

    let mut out = Vec::new();
    doc.save_to(&mut out)
        .map_err(|e| CommandError::PdfError(format!("lopdf save: {e}")))?;

    // The replace must not move or drop any image — only the targeted pixels change.
    let after = extract_images_from_bytes(&out, page)?;
    let placement_preserved = after.len() == before.len()
        && after
            .iter()
            .zip(&before)
            .all(|(a, b)| a.bbox.iter().zip(&b.bbox).all(|(x, y)| (x - y).abs() < 1.0));
    if !placement_preserved {
        return Err(CommandError::PdfError(
            "image replace disturbed the page's image geometry".to_owned(),
        ));
    }
    Ok(out)
}

/// Resolve the `/XObject` object id that the `index`-th image `Do` on the page
/// references. Walks the content `Do` operators (image-aware) to the Nth image,
/// then looks its resource name up in `/Resources /XObject`.
fn nth_image_xobject_id(doc: &Document, page_id: lopdf::ObjectId, index: usize) -> Option<lopdf::ObjectId> {
    let image_names = image_xobject_names(doc, page_id);
    let operations = doc.get_and_decode_page_content(page_id).ok()?.operations;

    let mut seen = 0usize;
    let mut name = None;
    for op in &operations {
        if op.operator == "Do" {
            if let Some(Object::Name(n)) = op.operands.first() {
                if image_names.contains(n) {
                    if seen == index {
                        name = Some(n.clone());
                        break;
                    }
                    seen += 1;
                }
            }
        }
    }
    let name = name?;

    let resources = deref_dict(doc, doc.get_dictionary(page_id).ok()?.get(b"Resources").ok()?)?;
    let xobjects = deref_dict(doc, resources.get(b"XObject").ok()?)?;
    xobjects.get(name.as_slice()).ok()?.as_reference().ok()
}

/// SPEC: P4-EDIT-006 — move/resize/rotate an image as one undoable edit. Snapshots
/// the live document, runs the bytes → bytes transform, swaps the document; the
/// inverse restores the pre-edit bytes. Mirrors [`crate::pdf::reflow`].
pub struct TransformImageEdit {
    pub page: usize,
    pub index: usize,
    pub matrix: [f32; 6],
}

impl<'a> Edit<PdfDocument<'a>> for TransformImageEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        image_edit_apply(doc, |bytes| transform_image(bytes, self.page, self.index, self.matrix))
    }

    fn label(&self) -> &'static str {
        "transform-image"
    }
}

/// SPEC: P4-EDIT-006 — delete an image as one undoable edit.
pub struct DeleteImageEdit {
    pub page: usize,
    pub index: usize,
}

impl<'a> Edit<PdfDocument<'a>> for DeleteImageEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        image_edit_apply(doc, |bytes| delete_image(bytes, self.page, self.index))
    }

    fn label(&self) -> &'static str {
        "delete-image"
    }
}

/// SPEC: P4-EDIT-006 — replace an image's pixels as one undoable edit.
pub struct ReplaceImageEdit {
    pub page: usize,
    pub index: usize,
    pub image: Vec<u8>,
}

impl<'a> Edit<PdfDocument<'a>> for ReplaceImageEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        image_edit_apply(doc, |bytes| replace_image(bytes, self.page, self.index, &self.image))
    }

    fn label(&self) -> &'static str {
        "replace-image"
    }
}

/// Snapshot the live document, run a bytes → bytes edit, reload/replace it; the
/// inverse is the pre-edit snapshot. (Same shape as `reflow`'s `reflow_edit`.)
fn image_edit_apply<'a>(
    doc: &mut PdfDocument<'a>,
    f: impl FnOnce(&[u8]) -> Result<Vec<u8>, CommandError>,
) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
    let pre_bytes = {
        let _guard = pdfium_lock()?;
        doc.save_to_bytes().map_err(CommandError::from)?
    };
    let new_bytes = f(&pre_bytes)?;
    {
        let _guard = pdfium_lock()?;
        *doc = pdfium()?
            .load_pdf_from_byte_vec(new_bytes, None)
            .map_err(CommandError::from)?;
    }
    Ok(Box::new(RestoreDocEdit { bytes: pre_bytes }))
}
