//! Embed a raster image as a PDF Image `XObject` (P3.C3b stamps; P4.C1 add-image).
//!
//! SPEC: P3-ANN-006 / P4-EDIT-005. Two encodings:
//! - **`PNG`** ([`embed_png`]) is decoded with the `png` crate (already a
//!   dependency) into raw samples → a `DeviceGray`/`DeviceRGB` Image `XObject`,
//!   with any alpha split out as a grayscale `/SMask`. Stored **uncompressed**
//!   (no `/Filter`) for simplicity.
//! - **`JPEG`** ([`embed_jpeg`]) is embedded **verbatim** as a `/DCTDecode`
//!   stream — `PDF` speaks JPEG natively, so there's no decode step; we only parse
//!   the `SOF` header for dimensions + component count (→ colour space).
//!
//! [`embed_image`] dispatches on the magic bytes. `GIF`/`BMP`/`TIFF`/`WebP` need a
//! raster decoder we don't bundle — they error cleanly (BACKLOG).

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

/// True if `bytes` looks like a JPEG (`FF D8 FF …`).
#[must_use]
pub fn is_jpeg(bytes: &[u8]) -> bool {
    bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF
}

/// SPEC: P4-EDIT-005 — embed a `JPEG` verbatim as a `/DCTDecode` Image `XObject`.
/// No pixel decode: we parse the `SOF` header for dimensions + component count
/// (the colour space) and store the original JPEG bytes as the stream content.
pub fn embed_jpeg(doc: &mut Document, bytes: &[u8]) -> Result<EmbeddedImage, CommandError> {
    if !is_jpeg(bytes) {
        return Err(CommandError::InvalidInput("image is not a JPEG".into()));
    }
    let JpegInfo { width, height, components } = parse_jpeg_sof(bytes)?;
    let color_space: &[u8] = match components {
        1 => b"DeviceGray",
        3 => b"DeviceRGB",
        4 => b"DeviceCMYK",
        n => return Err(CommandError::InvalidInput(format!("unsupported JPEG component count: {n}"))),
    };
    let mut dict = image_dict(width, height, color_space, None);
    // The stream *is* the DCTDecode-encoded data — keep lopdf from re-compressing it.
    dict.set("Filter", Object::Name(b"DCTDecode".to_vec()));
    // SPEC: P4-EDIT-005 — Adobe CMYK JPEGs (Photoshop's default; marked by an APP14
    // "Adobe" segment) store *inverted* samples. Without an inverting `/Decode` they
    // render as a dark negative — the "image background does nothing" bug. Add it so
    // CMYK images paint with correct colours; RGB/gray JPEGs are untouched.
    if components == 4 && jpeg_has_adobe_app14(bytes) {
        dict.set(
            "Decode",
            Object::Array(vec![
                Object::Integer(1),
                Object::Integer(0),
                Object::Integer(1),
                Object::Integer(0),
                Object::Integer(1),
                Object::Integer(0),
                Object::Integer(1),
                Object::Integer(0),
            ]),
        );
    }
    let id = doc.add_object(Stream::new(dict, bytes.to_vec()).with_compression(false));
    Ok(EmbeddedImage { id, width, height })
}

/// True if the JPEG carries an **Adobe APP14** marker (`FF EE … "Adobe"`). Adobe
/// CMYK JPEGs store inverted samples, so a 4-component one needs a `/Decode`
/// inversion to render correctly. Walks the marker segments up to the scan (`SOS`).
fn jpeg_has_adobe_app14(bytes: &[u8]) -> bool {
    let mut i = 2; // skip the SOI (FF D8)
    while i + 4 <= bytes.len() {
        if bytes[i] != 0xFF {
            return false; // not aligned on a marker → give up
        }
        let marker = bytes[i + 1];
        // Standalone markers (no length payload): RSTn (D0..D7), SOI/EOI, TEM.
        if (0xD0..=0xD9).contains(&marker) || marker == 0x01 {
            i += 2;
            continue;
        }
        if marker == 0xDA {
            return false; // start of scan — image data begins; no APP14 before it
        }
        let len = ((bytes[i + 2] as usize) << 8) | bytes[i + 3] as usize;
        if len < 2 {
            return false; // malformed segment length
        }
        if marker == 0xEE {
            let start = i + 4;
            let end = (i + 2 + len).min(bytes.len());
            if end > start && bytes[start..end].starts_with(b"Adobe") {
                return true;
            }
        }
        i += 2 + len;
    }
    false
}

/// SPEC: P4-EDIT-005 — embed `bytes` as an Image `XObject`, dispatching on the
/// magic bytes. PNG → `FlateDecode`-style raw samples, JPEG → `DCTDecode`; anything
/// else errors (the unsupported formats are a documented limitation).
pub fn embed_image(doc: &mut Document, bytes: &[u8]) -> Result<EmbeddedImage, CommandError> {
    if is_png(bytes) {
        embed_png(doc, bytes)
    } else if is_jpeg(bytes) {
        embed_jpeg(doc, bytes)
    } else {
        Err(CommandError::InvalidInput(
            "unsupported image format (only PNG and JPEG are supported)".into(),
        ))
    }
}

struct JpegInfo {
    width: u32,
    height: u32,
    components: u8,
}

/// Walk a JPEG's marker segments to its Start-Of-Frame (`SOF0`–`SOF15`, excluding
/// the non-frame `C4`/`C8`/`CC`) and read `[precision, height, width, components]`.
/// Pure byte scan — defensive against truncation; errors if no `SOF` is found.
fn parse_jpeg_sof(bytes: &[u8]) -> Result<JpegInfo, CommandError> {
    let bad = || CommandError::InvalidInput("JPEG header is malformed or has no SOF marker".into());
    let mut i = 2; // skip the SOI (FF D8)
    while i + 1 < bytes.len() {
        if bytes[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = bytes[i + 1];
        // Fill byte (FF FF) → advance one and retry.
        if marker == 0xFF {
            i += 1;
            continue;
        }
        // Standalone markers carry no length payload.
        if marker == 0xD8 || marker == 0xD9 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            i += 2;
            continue;
        }
        // Every other marker is followed by a 2-byte segment length.
        if i + 4 > bytes.len() {
            return Err(bad());
        }
        let len = usize::from(u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]));
        if len < 2 {
            return Err(bad());
        }
        let is_sof = (0xC0..=0xCF).contains(&marker) && marker != 0xC4 && marker != 0xC8 && marker != 0xCC;
        if is_sof {
            // Segment data: precision(1) height(2) width(2) components(1).
            if i + 9 >= bytes.len() {
                return Err(bad());
            }
            let height = u32::from(u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]));
            let width = u32::from(u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]));
            let components = bytes[i + 9];
            if width == 0 || height == 0 {
                return Err(bad());
            }
            return Ok(JpegInfo { width, height, components });
        }
        i += 2 + len;
    }
    Err(bad())
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

    /// A minimal JPEG byte stream: SOI + an APP0 segment + an `SOF0` frame header
    /// for a `width`×`height`, `components`-channel image + EOI. Enough for the
    /// header parser + `embed_jpeg` (which never decodes the entropy data).
    fn make_jpeg_header(width: u16, height: u16, components: u8) -> Vec<u8> {
        let mut out = vec![0xFF, 0xD8]; // SOI
        // APP0 (JFIF) — a length-bearing segment to skip past.
        out.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00]);
        // SOF0: marker, length, precision, height, width, components, (per-comp data omitted len-wise)
        out.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x0B, 0x08]);
        out.extend_from_slice(&height.to_be_bytes());
        out.extend_from_slice(&width.to_be_bytes());
        out.push(components);
        out.extend_from_slice(&[0x01, 0x11, 0x00]); // one component descriptor
        out.extend_from_slice(&[0xFF, 0xD9]); // EOI
        out
    }

    #[test]
    fn sniffs_jpeg_magic() {
        use super::is_jpeg;
        assert!(is_jpeg(&make_jpeg_header(2, 2, 3)));
        assert!(!is_jpeg(&make_png(2, 2, png::ColorType::Rgb)));
        assert!(!is_jpeg(b"\xff\xd8 short"));
    }

    #[test]
    fn jpeg_becomes_dctdecode_xobject_with_parsed_dims() {
        use super::embed_jpeg;
        let mut doc = Document::with_version("1.5");
        let img = embed_jpeg(&mut doc, &make_jpeg_header(640, 480, 3)).expect("embed jpeg");
        assert_eq!((img.width, img.height), (640, 480));
        let stream = doc.get_object(img.id).and_then(Object::as_stream).unwrap();
        assert_eq!(stream.dict.get(b"Filter").unwrap().as_name().unwrap(), b"DCTDecode");
        assert_eq!(stream.dict.get(b"ColorSpace").unwrap().as_name().unwrap(), b"DeviceRGB");
        assert_eq!(stream.dict.get(b"Width").unwrap().as_i64().unwrap(), 640);
    }

    #[test]
    fn embed_image_dispatches_and_rejects_unknown() {
        use super::embed_image;
        let mut doc = Document::with_version("1.5");
        assert!(embed_image(&mut doc, &make_png(2, 2, png::ColorType::Rgb)).is_ok());
        assert!(embed_image(&mut doc, &make_jpeg_header(2, 2, 1)).is_ok());
        // GIF magic — an unsupported format must error, not panic.
        assert!(embed_image(&mut doc, b"GIF89a\x01\x00").is_err());
    }

    #[test]
    fn deinterleave_separates_colour_and_alpha() {
        // Two RGBA pixels: (1,2,3,a=4) (5,6,7,a=8).
        let (color, alpha) = deinterleave(&[1, 2, 3, 4, 5, 6, 7, 8], 3);
        assert_eq!(color, vec![1, 2, 3, 5, 6, 7]);
        assert_eq!(alpha, vec![4, 8]);
    }

    /// `make_jpeg_header` plus an APP14 "Adobe" segment (marks CMYK data inverted).
    fn make_adobe_jpeg(width: u16, height: u16, components: u8) -> Vec<u8> {
        let mut out = vec![0xFF, 0xD8]; // SOI
        // APP14 (Adobe): len 14 = 2 + "Adobe"(5) + version/flags/transform(7).
        out.extend_from_slice(&[0xFF, 0xEE, 0x00, 0x0E]);
        out.extend_from_slice(b"Adobe");
        out.extend_from_slice(&[0x00, 0x64, 0x00, 0x00, 0x00, 0x00, 0x02]);
        out.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00]); // APP0
        out.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x0B, 0x08]); // SOF0
        out.extend_from_slice(&height.to_be_bytes());
        out.extend_from_slice(&width.to_be_bytes());
        out.push(components);
        out.extend_from_slice(&[0x01, 0x11, 0x00]);
        out.extend_from_slice(&[0xFF, 0xD9]); // EOI
        out
    }

    #[test]
    fn detects_adobe_app14_marker() {
        use super::jpeg_has_adobe_app14;
        assert!(jpeg_has_adobe_app14(&make_adobe_jpeg(2, 2, 4)), "APP14 Adobe present");
        assert!(!jpeg_has_adobe_app14(&make_jpeg_header(2, 2, 4)), "no APP14 → false");
    }

    /// SPEC: P4-EDIT-005 — an Adobe CMYK JPEG gets the inverting `/Decode`; RGB and
    /// non-Adobe CMYK do not (so we don't double-invert).
    #[test]
    fn cmyk_adobe_jpeg_gets_inverting_decode() {
        use super::embed_jpeg;
        let mut doc = Document::with_version("1.5");

        let img = embed_jpeg(&mut doc, &make_adobe_jpeg(8, 8, 4)).expect("embed cmyk");
        let stream = doc.get_object(img.id).and_then(Object::as_stream).unwrap();
        assert_eq!(stream.dict.get(b"ColorSpace").unwrap().as_name().unwrap(), b"DeviceCMYK");
        let decode = stream.dict.get(b"Decode").and_then(Object::as_array).expect("/Decode");
        let vals: Vec<i64> = decode.iter().map(|o| o.as_i64().unwrap()).collect();
        assert_eq!(vals, vec![1, 0, 1, 0, 1, 0, 1, 0]);

        let rgb = embed_jpeg(&mut doc, &make_adobe_jpeg(8, 8, 3)).expect("embed rgb");
        let rgb_s = doc.get_object(rgb.id).and_then(Object::as_stream).unwrap();
        assert!(rgb_s.dict.get(b"Decode").is_err(), "RGB Adobe JPEG needs no /Decode");

        let plain = embed_jpeg(&mut doc, &make_jpeg_header(8, 8, 4)).expect("embed plain cmyk");
        let plain_s = doc.get_object(plain.id).and_then(Object::as_stream).unwrap();
        assert!(plain_s.dict.get(b"Decode").is_err(), "non-Adobe CMYK: no forced inversion");
    }
}
