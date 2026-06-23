//! Embed a raster image as a PDF Image `XObject` (P3.C3b — image stamps).
//!
//! SPEC: P3-ANN-006 — custom stamps from an image. We decode a `PNG` with the
//! `png` crate (already a dependency — used as the render encoder; its decoder
//! ships in the same crate) and build a `/Subtype /Image` `XObject` the stamp
//! `/AP` can paint with `Do`. An alpha channel becomes a grayscale `/SMask` so a
//! transparent signature/logo stamps cleanly. `JPEG` + other formats are
//! deferred (BACKLOG); only `PNG` is accepted here.
//!
//! Pixel data is stored **uncompressed** (no `/Filter`) for v1 — simplest and
//! dependency-free; stamps are small. Flate-compressing the stream is a noted
//! follow-up.

use std::io::Cursor;

use lopdf::{Dictionary, Document, Object, ObjectId, Stream};

use crate::error::CommandError;

/// The 8-byte PNG signature.
const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// True if `bytes` begins with the PNG signature.
#[must_use]
pub fn is_png(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && bytes[..8] == PNG_MAGIC
}

/// A decoded image embedded into a document: the Image `XObject`'s id + its
/// pixel dimensions (for aspect-correct placement).
pub struct EmbeddedImage {
    pub id: ObjectId,
    pub width: u32,
    pub height: u32,
}

/// SPEC: P3-ANN-006 — decode `bytes` (`PNG`) and add an Image `XObject` (plus an
/// `/SMask` for any alpha) to `doc`, returning its id + dimensions. Errors
/// (non-`PNG`, unsupported depth) are typed, never a panic.
pub fn embed_png(doc: &mut Document, bytes: &[u8]) -> Result<EmbeddedImage, CommandError> {
    if !is_png(bytes) {
        return Err(CommandError::InvalidInput("stamp image is not a PNG".into()));
    }
    // Normalise palette/low-bit/16-bit down to 8-bit RGB(A) / Gray(A).
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(png_err)?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(png_err)?;
    buf.truncate(info.buffer_size());

    if info.bit_depth != png::BitDepth::Eight {
        return Err(CommandError::InvalidInput("unsupported PNG bit depth (need 8-bit)".into()));
    }
    let (width, height) = (info.width, info.height);

    // Split into colour samples + (optional) alpha, and pick the colour space.
    let (color_space, color, alpha): (&[u8], Vec<u8>, Option<Vec<u8>>) = match info.color_type {
        png::ColorType::Grayscale => (b"DeviceGray", buf, None),
        png::ColorType::Rgb => (b"DeviceRGB", buf, None),
        png::ColorType::GrayscaleAlpha => {
            let (c, a) = deinterleave(&buf, 1);
            (b"DeviceGray", c, Some(a))
        }
        png::ColorType::Rgba => {
            let (c, a) = deinterleave(&buf, 3);
            (b"DeviceRGB", c, Some(a))
        }
        png::ColorType::Indexed => {
            // normalize_to_color8 expands palettes, so this shouldn't occur.
            return Err(CommandError::InvalidInput("unexpected indexed PNG after expansion".into()));
        }
    };

    // The soft-mask image (grayscale alpha), added first so the image can ref it.
    let smask_id = alpha.map(|a| {
        let dict = image_dict(width, height, b"DeviceGray", None);
        doc.add_object(Stream::new(dict, a).with_compression(false))
    });

    let img_dict = image_dict(width, height, color_space, smask_id);
    let id = doc.add_object(Stream::new(img_dict, color).with_compression(false));
    Ok(EmbeddedImage { id, width, height })
}

/// The `XObject` dictionary for an 8-bit image with an optional `/SMask` ref.
fn image_dict(width: u32, height: u32, color_space: &[u8], smask: Option<ObjectId>) -> Dictionary {
    let mut dict = Dictionary::new();
    dict.set("Type", Object::Name(b"XObject".to_vec()));
    dict.set("Subtype", Object::Name(b"Image".to_vec()));
    dict.set("Width", Object::Integer(i64::from(width)));
    dict.set("Height", Object::Integer(i64::from(height)));
    dict.set("ColorSpace", Object::Name(color_space.to_vec()));
    dict.set("BitsPerComponent", Object::Integer(8));
    if let Some(id) = smask {
        dict.set("SMask", Object::Reference(id));
    }
    dict
}

/// Split interleaved `colour…alpha` pixels (each `color_components` colour bytes
/// then one alpha byte) into a packed colour buffer + a packed alpha buffer.
fn deinterleave(buf: &[u8], color_components: usize) -> (Vec<u8>, Vec<u8>) {
    let stride = color_components + 1;
    let pixels = buf.len() / stride;
    let mut color = Vec::with_capacity(pixels * color_components);
    let mut alpha = Vec::with_capacity(pixels);
    for px in buf.chunks_exact(stride) {
        color.extend_from_slice(&px[..color_components]);
        alpha.push(px[color_components]);
    }
    (color, alpha)
}

#[allow(clippy::needless_pass_by_value)]
fn png_err(e: png::DecodingError) -> CommandError {
    CommandError::InvalidInput(format!("invalid PNG: {e}"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::{deinterleave, embed_png, is_png};
    use lopdf::{Document, Object};

    /// Encode a tiny `width`×`height` image with the given `png` colour type.
    fn make_png(width: u32, height: u32, color: png::ColorType) -> Vec<u8> {
        let components = match color {
            png::ColorType::Grayscale => 1,
            png::ColorType::GrayscaleAlpha => 2,
            png::ColorType::Rgb => 3,
            png::ColorType::Rgba => 4,
            png::ColorType::Indexed => unreachable!(),
        };
        let data = vec![128u8; (width * height) as usize * components];
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, width, height);
            enc.set_color(color);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().unwrap();
            w.write_image_data(&data).unwrap();
        }
        out
    }

    #[test]
    fn sniffs_png_magic() {
        assert!(is_png(&make_png(2, 2, png::ColorType::Rgb)));
        assert!(!is_png(b"not a png"));
        assert!(!is_png(b""));
    }

    #[test]
    fn rgb_png_becomes_devicergb_xobject_without_smask() {
        let mut doc = Document::with_version("1.5");
        let img = embed_png(&mut doc, &make_png(4, 3, png::ColorType::Rgb)).expect("embed");
        assert_eq!((img.width, img.height), (4, 3));
        let stream = doc.get_object(img.id).and_then(Object::as_stream).unwrap();
        assert_eq!(stream.dict.get(b"ColorSpace").unwrap().as_name().unwrap(), b"DeviceRGB");
        assert!(stream.dict.get(b"SMask").is_err(), "no alpha → no soft mask");
        assert_eq!(stream.content.len(), 4 * 3 * 3, "raw RGB samples, uncompressed");
    }

    #[test]
    fn rgba_png_splits_alpha_into_smask() {
        let mut doc = Document::with_version("1.5");
        let img = embed_png(&mut doc, &make_png(4, 3, png::ColorType::Rgba)).expect("embed");
        let stream = doc.get_object(img.id).and_then(Object::as_stream).unwrap();
        assert_eq!(stream.content.len(), 4 * 3 * 3, "colour buffer is RGB only");
        let smask_ref = stream.dict.get(b"SMask").and_then(Object::as_reference).expect("/SMask");
        let smask = doc.get_object(smask_ref).and_then(Object::as_stream).unwrap();
        assert_eq!(smask.dict.get(b"ColorSpace").unwrap().as_name().unwrap(), b"DeviceGray");
        assert_eq!(smask.content.len(), 4 * 3, "one alpha byte per pixel");
    }

    #[test]
    fn rejects_non_png() {
        let mut doc = Document::with_version("1.5");
        assert!(embed_png(&mut doc, b"\xff\xd8\xff\xe0 jpeg-ish").is_err());
        assert!(embed_png(&mut doc, b"").is_err());
    }

    #[test]
    fn deinterleave_separates_colour_and_alpha() {
        // Two RGBA pixels: (1,2,3,a=4) (5,6,7,a=8).
        let (color, alpha) = deinterleave(&[1, 2, 3, 4, 5, 6, 7, 8], 3);
        assert_eq!(color, vec![1, 2, 3, 5, 6, 7]);
        assert_eq!(alpha, vec![4, 8]);
    }
}
