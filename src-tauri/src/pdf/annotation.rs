//! Text-markup annotations as an undoable edit (P3.B1b).
//!
//! SPEC: P3-ANN-001 — write a standard PDF text-markup annotation
//! (highlight / underline / strikethrough / squiggly) over selected-text quads.
//! Done via the lopdf COS layer ([`crate::pdf::cos::add_text_markup`]) on a byte
//! round-trip — `PDFium` can't set annotation colour, and lopdf gives full control
//! over `/QuadPoints`, `/C`, and the `/AP` appearance. Same shape as resize:
//! serialize → cos edit → reload, replacing the actor's document. The inverse is
//! a pre-write snapshot ([`RestoreDocEdit`]).

use pdfium_render::prelude::PdfDocument;

use crate::error::CommandError;
use crate::pdf::cos::{
    add_free_text, add_ink, add_line, add_polygon, add_shape, add_stamp, add_text_markup,
    add_text_note,
    clear_text_markup, delete_annotation, update_free_text, update_text_note,
};
use crate::pdf::document::{pdfium, pdfium_lock};
use crate::pdf::restore::RestoreDocEdit;
use crate::pdf::undo::Edit;

/// Apply a pure `&[u8] → Vec<u8>` cos transform to the live document as an
/// undoable edit: snapshot the bytes (the inverse), run `f`, reload/replace the
/// document. Shared by the note add/update/delete edits.
fn cos_edit<'a>(
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

/// Add a text-markup annotation over `quads` (each `[x1..y4]` PDF pts) on `page`.
pub struct TextMarkupEdit {
    pub page: i32,
    pub subtype: String,
    pub quads: Vec<[f32; 8]>,
    pub color: String,
    pub opacity: f32,
}

impl<'a> Edit<PdfDocument<'a>> for TextMarkupEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        if self.quads.is_empty() {
            return Err(CommandError::InvalidInput("no quads for text markup".into()));
        }
        let page = usize::try_from(self.page)
            .map_err(|_| CommandError::InvalidInput(format!("negative page index: {}", self.page)))?;

        // 1. Snapshot the pre-write document — the lossless inverse.
        let pre_bytes = {
            let _guard = pdfium_lock()?;
            doc.save_to_bytes().map_err(CommandError::from)?
        };

        // 2. Write the annotation via lopdf (pure Rust, no lock). Validates the
        //    page + subtype, so a bad request fails here without touching `doc`.
        let new_bytes = add_text_markup(
            &pre_bytes,
            page,
            &self.subtype,
            &self.quads,
            &self.color,
            self.opacity,
        )?;

        // 3. Reload, replacing the actor's document (the reload is the round-trip
        //    verification — a malformed result would fail to open here).
        {
            let _guard = pdfium_lock()?;
            *doc = pdfium()?
                .load_pdf_from_byte_vec(new_bytes, None)
                .map_err(CommandError::from)?;
        }

        Ok(Box::new(RestoreDocEdit { bytes: pre_bytes }))
    }

    fn label(&self) -> &'static str {
        "text-markup"
    }
}

/// Remove every text-markup annotation from the document (P3.B1b).
pub struct ClearMarkupEdit;

impl<'a> Edit<PdfDocument<'a>> for ClearMarkupEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        let pre_bytes = {
            let _guard = pdfium_lock()?;
            doc.save_to_bytes().map_err(CommandError::from)?
        };
        let new_bytes = clear_text_markup(&pre_bytes)?;
        {
            let _guard = pdfium_lock()?;
            *doc = pdfium()?
                .load_pdf_from_byte_vec(new_bytes, None)
                .map_err(CommandError::from)?;
        }
        Ok(Box::new(RestoreDocEdit { bytes: pre_bytes }))
    }

    fn label(&self) -> &'static str {
        "clear-markup"
    }
}

/// SPEC: P3-ANN-002 — add a sticky note (`/Text` annotation) at `(x, y)`.
pub struct AddNoteEdit {
    pub note_id: String,
    pub page: i32,
    pub x: f32,
    pub y: f32,
    pub content: String,
    pub author: String,
}

impl<'a> Edit<PdfDocument<'a>> for AddNoteEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        let page = usize::try_from(self.page)
            .map_err(|_| CommandError::InvalidInput(format!("negative page index: {}", self.page)))?;
        cos_edit(doc, |bytes| {
            add_text_note(bytes, &self.note_id, page, self.x, self.y, &self.content, &self.author)
        })
    }

    fn label(&self) -> &'static str {
        "add-note"
    }
}

/// SPEC: P3-ANN-002 — update a note's body (`/Contents`) by `/NM`.
pub struct UpdateNoteEdit {
    pub note_id: String,
    pub content: String,
}

impl<'a> Edit<PdfDocument<'a>> for UpdateNoteEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        cos_edit(doc, |bytes| update_text_note(bytes, &self.note_id, &self.content))
    }

    fn label(&self) -> &'static str {
        "update-note"
    }
}

/// SPEC: P3-ANN-002 — delete the annotation with `/NM == note_id`.
pub struct DeleteAnnotationEdit {
    pub note_id: String,
}

impl<'a> Edit<PdfDocument<'a>> for DeleteAnnotationEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        cos_edit(doc, |bytes| delete_annotation(bytes, &self.note_id))
    }

    fn label(&self) -> &'static str {
        "delete-annotation"
    }
}

/// SPEC: P3-ANN-003 — add a free-text annotation (a `/FreeText` box with a
/// generated `/AP`) at `rect` on `page`. Uniform style; underline/rich text B3b.
pub struct FreeTextEdit {
    pub page: i32,
    pub rect: [f32; 4],
    pub text: String,
    pub font_family: String,
    pub font_size: f32,
    pub color: String,
    pub bold: bool,
    pub italic: bool,
}

impl<'a> Edit<PdfDocument<'a>> for FreeTextEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        let page = usize::try_from(self.page)
            .map_err(|_| CommandError::InvalidInput(format!("negative page index: {}", self.page)))?;
        cos_edit(doc, |bytes| {
            add_free_text(
                bytes,
                page,
                self.rect,
                &self.text,
                &self.font_family,
                self.font_size,
                &self.color,
                self.bold,
                self.italic,
            )
        })
    }

    fn label(&self) -> &'static str {
        "free-text"
    }
}

/// SPEC: P3-ANN-004 — add a shape annotation (`/Square` or `/Circle`) at `rect`
/// on `page` with a generated `/AP`.
pub struct ShapeEdit {
    pub page: i32,
    pub kind: String,
    pub rect: [f32; 4],
    pub stroke: String,
    pub fill: Option<String>,
    pub opacity: f32,
    pub stroke_width: f32,
}

impl<'a> Edit<PdfDocument<'a>> for ShapeEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        let page = usize::try_from(self.page)
            .map_err(|_| CommandError::InvalidInput(format!("negative page index: {}", self.page)))?;
        cos_edit(doc, |bytes| {
            add_shape(
                bytes,
                page,
                &self.kind,
                self.rect,
                &self.stroke,
                self.fill.as_deref(),
                self.opacity,
                self.stroke_width,
            )
        })
    }

    fn label(&self) -> &'static str {
        "shape"
    }
}

/// SPEC: P3-ANN-004 — add a line (or arrow) annotation with a generated `/AP`.
pub struct LineEdit {
    pub page: i32,
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
    pub arrow: bool,
    pub stroke: String,
    pub opacity: f32,
    pub stroke_width: f32,
}

impl<'a> Edit<PdfDocument<'a>> for LineEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        let page = usize::try_from(self.page)
            .map_err(|_| CommandError::InvalidInput(format!("negative page index: {}", self.page)))?;
        cos_edit(doc, |bytes| {
            add_line(
                bytes,
                page,
                self.x1,
                self.y1,
                self.x2,
                self.y2,
                self.arrow,
                &self.stroke,
                self.opacity,
                self.stroke_width,
            )
        })
    }

    fn label(&self) -> &'static str {
        "line"
    }
}

/// SPEC: P3-ANN-004 — add a polygon / polyline annotation with a generated `/AP`.
pub struct PolygonEdit {
    pub page: i32,
    pub closed: bool,
    pub points: Vec<[f32; 2]>,
    pub stroke: String,
    pub fill: Option<String>,
    pub opacity: f32,
    pub stroke_width: f32,
}

impl<'a> Edit<PdfDocument<'a>> for PolygonEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        let page = usize::try_from(self.page)
            .map_err(|_| CommandError::InvalidInput(format!("negative page index: {}", self.page)))?;
        cos_edit(doc, |bytes| {
            add_polygon(
                bytes,
                page,
                self.closed,
                &self.points,
                &self.stroke,
                self.fill.as_deref(),
                self.opacity,
                self.stroke_width,
            )
        })
    }

    fn label(&self) -> &'static str {
        "polygon"
    }
}

/// SPEC: P3-ANN-005 — add a freehand `/Ink` annotation with a generated `/AP`.
pub struct InkEdit {
    pub page: i32,
    /// `[x, y, pressure]` in page coords; smoothing applied frontend-side.
    pub points: Vec<[f32; 3]>,
    pub color: String,
    pub opacity: f32,
    pub base_width: f32,
}

impl<'a> Edit<PdfDocument<'a>> for InkEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        let page = usize::try_from(self.page)
            .map_err(|_| CommandError::InvalidInput(format!("negative page index: {}", self.page)))?;
        cos_edit(doc, |bytes| {
            add_ink(bytes, page, &self.points, &self.color, self.opacity, self.base_width)
        })
    }

    fn label(&self) -> &'static str {
        "ink"
    }
}

/// SPEC: P3-ANN-006 — add a `/Stamp` annotation with a generated `/AP`.
pub struct StampEdit {
    pub page: i32,
    pub rect: [f32; 4],
    pub text: String,
    pub name: String,
    pub color: String,
    pub opacity: f32,
}

impl<'a> Edit<PdfDocument<'a>> for StampEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        let page = usize::try_from(self.page)
            .map_err(|_| CommandError::InvalidInput(format!("negative page index: {}", self.page)))?;
        cos_edit(doc, |bytes| {
            add_stamp(bytes, page, self.rect, &self.text, &self.name, &self.color, self.opacity)
        })
    }

    fn label(&self) -> &'static str {
        "stamp"
    }
}

/// SPEC: P3-ANN-013 — update a free-text annotation (by `/NM`) in place.
pub struct UpdateFreeTextEdit {
    pub nm: String,
    pub text: String,
    pub font_family: String,
    pub font_size: f32,
    pub color: String,
    pub bold: bool,
    pub italic: bool,
}

impl<'a> Edit<PdfDocument<'a>> for UpdateFreeTextEdit {
    fn apply(
        self: Box<Self>,
        doc: &mut PdfDocument<'a>,
    ) -> Result<Box<dyn Edit<PdfDocument<'a>>>, CommandError> {
        cos_edit(doc, |bytes| {
            update_free_text(
                bytes,
                &self.nm,
                &self.text,
                &self.font_family,
                self.font_size,
                &self.color,
                self.bold,
                self.italic,
            )
        })
    }

    fn label(&self) -> &'static str {
        "update-free-text"
    }
}
