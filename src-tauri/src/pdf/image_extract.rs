//! Image-object location (P4.C2) — the read foundation of image editing.
//!
//! SPEC: P4-EDIT-006 — "WHEN the user clicks an existing image…". To hit-test a
//! click and draw a selection box, the frontend first needs *where each image is*.
//! This walks a page's `PDFium` **image page-objects** (the `Do`-painted Image
//! `XObject`s) and emits an [`ImageInfo`] per object: its ordinal index, page-space
//! bounding box, and placement matrix. Read-only; on the live `PdfDocument` under
//! the `PDFium` lock, exactly like [`crate::pdf::text_extract`].

use pdfium_render::prelude::*;

use crate::error::CommandError;
use crate::pdf::document::{pdfium, pdfium_lock};

/// One image on a page, as the frontend's click-to-select hit-test consumes it.
/// `index` is the image ordinal (the Nth image object, counting in page-object
/// order) — the handle the edit/delete commands take, mirroring text `run_index`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageInfo {
    /// 0-based image ordinal on the page.
    pub index: u32,
    /// Axis-aligned bounds `[x0, y0, x1, y1]` in PDF points (origin bottom-left).
    pub bbox: [f32; 4],
    /// The image's placement matrix `[a, b, c, d, e, f]` (maps the unit square).
    pub matrix: [f32; 6],
}

/// SPEC: P4-EDIT-006 — extract every image on `page` (0-based) of the live
/// document. Read-only; acquires the shared `PDFium` lock (like `render_page`). An
/// image whose geometry can't be read is skipped, not failed.
pub fn extract_images(doc: &PdfDocument, page: usize) -> Result<Vec<ImageInfo>, CommandError> {
    let index = i32::try_from(page)
        .map_err(|_| CommandError::InvalidInput(format!("bad page index: {page}")))?;
    let _guard = pdfium_lock()?;
    let pdf_page = doc.pages().get(index).map_err(CommandError::from)?;

    let mut images = Vec::new();
    let mut ordinal = 0u32;
    for object in pdf_page.objects().iter() {
        let PdfPageObject::Image(image_object) = &object else {
            continue; // text / paths are not images
        };
        let Ok(bounds) = image_object.bounds() else {
            continue; // no usable geometry → can't place a selection box
        };
        let bbox = [
            bounds.left().value,
            bounds.bottom().value,
            bounds.right().value,
            bounds.top().value,
        ];
        let matrix = image_object.matrix().map_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0], |m| {
            [m.a(), m.b(), m.c(), m.d(), m.e(), m.f()]
        });
        images.push(ImageInfo { index: ordinal, bbox, matrix });
        ordinal += 1;
    }
    Ok(images)
}

/// Load `bytes` into a throwaway `PDFium` document and extract `page`'s images.
/// Used by the image-edit verify path (and tests) where there's no live document
/// handle. Loading needs the lock; `extract_images` re-acquires it, so it isn't
/// held across the call; the throwaway doc is closed under the lock.
pub fn extract_images_from_bytes(bytes: &[u8], page: usize) -> Result<Vec<ImageInfo>, CommandError> {
    let doc = {
        let _guard = pdfium_lock()?;
        pdfium()?
            .load_pdf_from_byte_vec(bytes.to_vec(), None)
            .map_err(CommandError::from)?
    };
    let images = extract_images(&doc, page)?;
    {
        let _guard = pdfium_lock()?;
        drop(doc);
    }
    Ok(images)
}
