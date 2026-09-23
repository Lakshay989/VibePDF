//! SPEC: P7-OCR-010 — make a PDF smaller, at one of three levels.
//!
//! Two of the spec's three targets are implemented here: **stream deflation**
//! and **image recompression**. Font subsetting is deliberately not (decided
//! 2026-09-23) — doing it properly means walking every content stream to learn
//! which glyphs are actually used, and it saves nothing at all on the scanned
//! documents this feature exists for. It is its own step.
//!
//! **Where the bytes are.** Measured 2026-09-23 on a 20-page 300 DPI grayscale
//! scan with sensor noise (81.6 MB, built by `generate-noisy-scan.py`):
//!
//! | | 300 DPI | 200 DPI | 150 DPI |
//! |---|---|---|---|
//! | JPEG q85 | 20.4% | 9.1% | 5.2% |
//! | JPEG q60 | 5.6% | 2.5% | 1.5% |
//!
//! Stream deflation is the opposite shape — worth nothing on a scan (every
//! image stream is already Flate) and worth almost everything on a document
//! that embeds an uncompressed font: `unicode-text.pdf` goes 337 KB → 24 KB,
//! 92.8%, from deflation alone.
//!
//! **Why this is `lopdf` and not `PDFium`.** `PdfPageImageObject::set_image`
//! calls `FPDFImageObj_SetBitmap`, which stores a raw BGRA bitmap: it would
//! inflate every image fourfold and turn a `DeviceGray` scan into four
//! channels. `PDFium`'s only compressing path, `FPDFImageObj_LoadJpegFileInline`,
//! is reachable through `pdfium-render` solely when *creating* an object, not
//! when replacing one.
//!
//! **The two rules that keep this safe.** Nothing is replaced unless the
//! replacement is smaller, and nothing is touched whose colour cannot survive
//! the trip — see [`eligibility`]. A compressed file that is bigger, or whose
//! colours have shifted, is a worse failure than one that did not compress.

use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ExtendedColorType, GrayImage, ImageEncoder, RgbImage};
use lopdf::{Document, Object, ObjectId};
use serde::{Deserialize, Serialize};

use crate::error::CommandError;
use crate::pdf::cos::effective_media_box;

#[allow(clippy::needless_pass_by_value)]
fn cos_err(e: lopdf::Error) -> CommandError {
    CommandError::PdfError(format!("compress: {e}"))
}

/// SPEC: P7-OCR-010 — the three levels, as the spec names them.
///
/// Each is a (downsample target, JPEG quality) pair chosen from the measured
/// table in the module docs, so the promise each one makes is a number someone
/// checked rather than a word someone liked.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CompressLevel {
    /// Keep every pixel; re-encode at high quality. ~20% of a 300 DPI scan.
    /// Nothing visible at any zoom.
    Low,
    /// Down to 200 DPI at the same quality. ~9%. Nothing visible at 100% —
    /// a 200 DPI page is still nearly three times a screen's resolution.
    Medium,
    /// Down to 150 DPI at lower quality. ~1.5%. Text stays sharp; photographs
    /// soften.
    High,
}

impl CompressLevel {
    /// The resolution to downsample images to, or `None` to keep them as they
    /// are.
    #[must_use]
    pub fn target_dpi(self) -> Option<f32> {
        match self {
            Self::Low => None,
            Self::Medium => Some(200.0),
            Self::High => Some(150.0),
        }
    }

    /// JPEG quality, 1-100.
    #[must_use]
    pub fn jpeg_quality(self) -> u8 {
        match self {
            Self::Low | Self::Medium => 85,
            Self::High => 60,
        }
    }

    /// Parse the wire value.
    ///
    /// # Errors
    /// When `name` is not one of the three levels.
    pub fn parse(name: &str) -> Result<Self, CommandError> {
        match name {
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            other => Err(CommandError::InvalidInput(format!(
                "unknown compression level: {other}"
            ))),
        }
    }
}

/// What a compression run did, for the caller and for the UI's before/after.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompressReport {
    pub before_bytes: u64,
    pub after_bytes: u64,
    /// Images replaced with a smaller JPEG.
    pub images_recompressed: u32,
    /// Images left exactly as they were — ineligible, or already smaller than
    /// anything we could produce.
    pub images_untouched: u32,
    /// Streams that gained a `/FlateDecode` filter.
    pub streams_deflated: u32,
}

impl CompressReport {
    /// Bytes saved, saturating at zero: a run that could not help returns the
    /// original file, never a negative saving.
    #[must_use]
    pub fn saved_bytes(&self) -> u64 {
        self.before_bytes.saturating_sub(self.after_bytes)
    }
}

/// Why an image was left alone. Not serialized — it exists so the skip
/// decisions are named rather than buried in a chain of `if`s, and so the tests
/// can assert on the reason rather than on "nothing happened".
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ineligible {
    /// Not an image `XObject` at all.
    NotAnImage,
    /// `/JPXDecode`: decoding JPEG 2000 would mean another dependency.
    Jpeg2000,
    /// 1 bit a sample. These are CCITT or JBIG2 bilevel scans: already tiny,
    /// and JPEG is the wrong codec for them — it would add grey fringes to
    /// pure black-on-white and usually grow the file.
    Bilevel,
    /// Anything but 8 bits a component. 16-bit would lose precision silently.
    BitDepth,
    /// `/Indexed`, `/Separation`, `/DeviceN`, or a colour space we cannot name.
    /// A palette index or an ink amount is not a colour value; re-encoding it
    /// as one changes what the page means, not just how it looks.
    ColourSpace,
    /// A `/Mask`. Colour-key masking selects pixels by exact sample value, and
    /// JPEG changes sample values — the mask would start hiding the wrong
    /// pixels. (`/SMask` is fine: it is a separate image and is left alone.)
    Masked,
    /// Too few pixels for the re-encode to be worth the JPEG header.
    TooSmall,
}

/// How the samples in a stream are laid out, once we have decided we can read
/// them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Samples {
    Gray,
    Rgb,
}

/// Below this, a JPEG's own headers dominate and there is nothing to win.
const MIN_PIXELS: u32 = 64 * 64;

/// SPEC: P7-OCR-010 — decide whether an image stream can be re-encoded without
/// changing what the page means.
///
/// An allowlist, not a denylist: an unrecognised colour space is refused rather
/// than guessed at.
///
/// The colour space is read only to learn **how many channels** the samples
/// have. The `/ColorSpace` entry itself is never rewritten — a `DCTDecode`
/// stream is interpreted through whatever colour space the image dictionary
/// names, so an ICC-managed or calibrated image keeps its profile and its
/// rendering across the re-encode. That is why `ICCBased` is allowed here
/// rather than refused: nothing about it is being dropped.
fn eligibility(doc: &Document, dict: &lopdf::Dictionary) -> Result<Samples, Ineligible> {
    if dict.get(b"Subtype").and_then(Object::as_name).unwrap_or(b"") != b"Image" {
        return Err(Ineligible::NotAnImage);
    }
    if dict.has(b"Mask") {
        return Err(Ineligible::Masked);
    }

    let filters: Vec<&[u8]> = match dict.get(b"Filter") {
        Ok(Object::Name(n)) => vec![n.as_slice()],
        Ok(Object::Array(a)) => a.iter().filter_map(|o| o.as_name().ok()).collect(),
        _ => Vec::new(),
    };
    if filters.iter().any(|f| *f == b"JPXDecode") {
        return Err(Ineligible::Jpeg2000);
    }

    let bpc = dict.get(b"BitsPerComponent").and_then(Object::as_i64).unwrap_or(8);
    if bpc == 1 {
        return Err(Ineligible::Bilevel);
    }
    if bpc != 8 {
        return Err(Ineligible::BitDepth);
    }

    let width = dict.get(b"Width").and_then(Object::as_i64).unwrap_or(0);
    let height = dict.get(b"Height").and_then(Object::as_i64).unwrap_or(0);
    if width <= 0 || height <= 0 || (width * height) < i64::from(MIN_PIXELS) {
        return Err(Ineligible::TooSmall);
    }

    let space = dict
        .get(b"ColorSpace")
        .and_then(|o| doc.dereference(o).map(|(_, o)| o))
        .map_err(|_| Ineligible::ColourSpace)?;
    channels_of(doc, space)
}

/// The channel count a colour space implies, or why we will not guess.
fn channels_of(doc: &Document, space: &Object) -> Result<Samples, Ineligible> {
    match space {
        Object::Name(n) => match n.as_slice() {
            b"DeviceGray" | b"CalGray" | b"G" => Ok(Samples::Gray),
            b"DeviceRGB" | b"CalRGB" | b"RGB" => Ok(Samples::Rgb),
            // DeviceCMYK is four-channel; JPEG can hold it but the Adobe
            // inversion convention makes a round-trip easy to get subtly
            // wrong, and CMYK scans are rare. Refused rather than guessed.
            _ => Err(Ineligible::ColourSpace),
        },
        Object::Array(a) => match a.first().and_then(|o| o.as_name().ok()) {
            // [/ICCBased stream] — /N in the stream says how many channels.
            Some(b"ICCBased") => {
                let n = a
                    .get(1)
                    .and_then(|o| doc.dereference(o).ok())
                    .and_then(|(_, o)| o.as_stream().ok())
                    .and_then(|s| s.dict.get(b"N").and_then(Object::as_i64).ok());
                match n {
                    Some(1) => Ok(Samples::Gray),
                    Some(3) => Ok(Samples::Rgb),
                    _ => Err(Ineligible::ColourSpace),
                }
            }
            // [/CalGray <<..>>] / [/CalRGB <<..>>] are device spaces with a
            // white point; the sample layout is the same.
            Some(b"CalGray") => Ok(Samples::Gray),
            Some(b"CalRGB") => Ok(Samples::Rgb),
            // /Indexed is a palette lookup and /Separation and /DeviceN are ink
            // amounts. In all three a sample is an *index* or a *quantity*, not
            // a colour, so a lossy re-encode changes which colour is named,
            // not how precisely it is named.
            _ => Err(Ineligible::ColourSpace),
        },
        _ => Err(Ineligible::ColourSpace),
    }
}

/// The page width in points for every image `XObject` the document draws.
///
/// Needed to turn "200 DPI" into a pixel count. The true resolution of an image
/// is its pixel width over the width it is *drawn* at, which lives in a `cm`
/// matrix inside a content stream; this uses the **page** width instead, which
/// is exact for the full-page scans the feature exists for and an
/// underestimate everywhere else.
///
/// Underestimating is the safe direction: a 2000-pixel logo drawn one inch wide
/// on a Letter page reads as 235 DPI rather than 2000, so it is left alone
/// instead of being downsampled to a blur. The cost is that such an image does
/// not shrink; the alternative error would damage it.
fn page_widths(doc: &Document) -> std::collections::HashMap<ObjectId, f32> {
    let mut widths: std::collections::HashMap<ObjectId, f32> = std::collections::HashMap::new();
    for (_, page_id) in doc.get_pages() {
        let Some(box_) = effective_media_box(doc, page_id) else {
            continue;
        };
        let width = (box_[2] - box_[0]).abs();
        if width <= 0.0 {
            continue;
        }
        let Ok((Some(resources), _)) = doc.get_page_resources(page_id) else {
            continue;
        };
        let Ok(xobjects) = resources.get(b"XObject").and_then(Object::as_dict) else {
            continue;
        };
        for (_, value) in xobjects {
            if let Ok(id) = value.as_reference() {
                // A shared image lands on several pages; the narrowest page
                // wins, keeping the DPI estimate an underestimate there too.
                widths
                    .entry(id)
                    .and_modify(|w| *w = w.min(width))
                    .or_insert(width);
            }
        }
    }
    widths
}

/// Decode an image stream's samples into something we can resize and re-encode.
///
/// `lopdf` undoes `/FlateDecode` and friends for us; a `/DCTDecode` stream is
/// already a JPEG and goes through the JPEG decoder instead.
fn decode(stream: &lopdf::Stream, kind: Samples) -> Option<DynamicImage> {
    let dict = &stream.dict;
    let width = u32::try_from(dict.get(b"Width").and_then(Object::as_i64).ok()?).ok()?;
    let height = u32::try_from(dict.get(b"Height").and_then(Object::as_i64).ok()?).ok()?;

    let is_jpeg = match dict.get(b"Filter") {
        Ok(Object::Name(n)) => n == b"DCTDecode",
        Ok(Object::Array(a)) => a.iter().filter_map(|o| o.as_name().ok()).any(|f| f == b"DCTDecode"),
        _ => false,
    };
    if is_jpeg {
        return image::load_from_memory_with_format(&stream.content, image::ImageFormat::Jpeg).ok();
    }

    let samples = stream.decompressed_content().ok()?;
    match kind {
        Samples::Gray => {
            let expected = (width as usize).checked_mul(height as usize)?;
            if samples.len() < expected {
                return None;
            }
            GrayImage::from_raw(width, height, samples[..expected].to_vec()).map(DynamicImage::ImageLuma8)
        }
        Samples::Rgb => {
            let expected = (width as usize).checked_mul(height as usize)?.checked_mul(3)?;
            if samples.len() < expected {
                return None;
            }
            RgbImage::from_raw(width, height, samples[..expected].to_vec()).map(DynamicImage::ImageRgb8)
        }
    }
}

/// Encode as a JPEG with the channel count the page expects.
///
/// The channel count must match what `/ColorSpace` says — the dictionary is not
/// rewritten, so a one-channel space has to receive a one-channel JPEG.
fn encode_jpeg(image: &DynamicImage, kind: Samples, quality: u8) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let encoder = JpegEncoder::new_with_quality(Cursor::new(&mut out), quality);
    let result = match kind {
        Samples::Gray => {
            let buf = image.to_luma8();
            encoder.write_image(buf.as_raw(), buf.width(), buf.height(), ExtendedColorType::L8)
        }
        Samples::Rgb => {
            let buf = image.to_rgb8();
            encoder.write_image(buf.as_raw(), buf.width(), buf.height(), ExtendedColorType::Rgb8)
        }
    };
    result.ok().map(|()| out)
}

/// The pixel width an image should have to sit at `target_dpi` on a page
/// `page_width_points` wide, or `None` when it is already at or below that.
fn downsampled_width(pixels: u32, page_width_points: f32, target_dpi: f32) -> Option<u32> {
    if page_width_points <= 0.0 {
        return None;
    }
    let inches = page_width_points / 72.0;
    #[allow(clippy::cast_precision_loss)]
    let current_dpi = pixels as f32 / inches;
    if current_dpi <= target_dpi {
        return None;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let wanted = (target_dpi * inches).round().max(1.0) as u32;
    (wanted < pixels).then_some(wanted)
}

/// SPEC: P7-OCR-010 — compress `bytes` at `level`, returning the new file and
/// what was done to it.
///
/// Images first, then stream deflation: re-encoding sets `/Filter /DCTDecode`,
/// and the deflation pass skips anything that already has a filter, so the
/// order stops it trying to zip a JPEG.
///
/// # Errors
/// When the input cannot be parsed or the result cannot be serialized.
pub fn compress_bytes(
    bytes: &[u8],
    level: CompressLevel,
) -> Result<(Vec<u8>, CompressReport), CommandError> {
    let mut doc = Document::load_mem(bytes).map_err(cos_err)?;
    let widths = page_widths(&doc);
    let mut report = CompressReport {
        before_bytes: bytes.len() as u64,
        ..CompressReport::default()
    };

    let ids: Vec<ObjectId> = doc.objects.keys().copied().collect();
    for id in ids {
        let Some(Object::Stream(stream)) = doc.objects.get(&id) else {
            continue;
        };
        let kind = match eligibility(&doc, &stream.dict) {
            Ok(kind) => kind,
            Err(Ineligible::NotAnImage) => continue,
            Err(_) => {
                report.images_untouched += 1;
                continue;
            }
        };

        let original_len = stream.content.len();
        let Some(image) = decode(stream, kind) else {
            report.images_untouched += 1;
            continue;
        };

        // Downsample first: fewer pixels is both the bigger saving and less
        // work for the encoder.
        let resized = level
            .target_dpi()
            .and_then(|dpi| {
                widths
                    .get(&id)
                    .and_then(|w| downsampled_width(image.width(), *w, dpi))
            })
            .map(|new_width| {
                let new_height =
                    (u64::from(image.height()) * u64::from(new_width) / u64::from(image.width()))
                        .max(1);
                // Triangle (bilinear), not Lanczos3. Measured 2026-09-23 on a
                // 2550x3300 scan page downsampled to 200 DPI: Triangle took
                // 3.3 s against Lanczos3's 6.7 s *and* produced a 21% smaller
                // JPEG (241 KB against 305 KB). Lanczos3 rings, which preserves
                // exactly the sensor noise the JPEG then has to spend bits on;
                // a bilinear kernel low-passes it away. Faster and smaller on
                // the same input is not a trade-off.
                #[allow(clippy::cast_possible_truncation)]
                image.resize_exact(
                    new_width,
                    new_height as u32,
                    image::imageops::FilterType::Triangle,
                )
            });
        let image = resized.as_ref().unwrap_or(&image);

        let Some(jpeg) = encode_jpeg(image, kind, level.jpeg_quality()) else {
            report.images_untouched += 1;
            continue;
        };

        // The rule that makes this safe to run on anything: an image already
        // stored more tightly than we can manage keeps its own bytes. Without
        // it, compressing a file twice makes it bigger the second time.
        if jpeg.len() >= original_len {
            report.images_untouched += 1;
            continue;
        }

        let (new_width, new_height) = (image.width(), image.height());
        let Some(Object::Stream(stream)) = doc.objects.get_mut(&id) else {
            continue;
        };
        stream.set_content(jpeg);
        stream.dict.set("Filter", Object::Name(b"DCTDecode".to_vec()));
        stream.dict.set("Width", i64::from(new_width));
        stream.dict.set("Height", i64::from(new_height));
        stream.dict.set("BitsPerComponent", 8_i64);
        // Decode parameters describe the *old* filter's framing (predictors,
        // colours a row); leaving them attached to a JPEG misleads a reader.
        stream.dict.remove(b"DecodeParms");
        stream.dict.remove(b"DP");
        // `/ColorSpace` is deliberately untouched — see `eligibility`.
        report.images_recompressed += 1;
    }

    // Stream deflation. `Stream::compress` already refuses a stream that has a
    // filter, and refuses a result that is not at least 19 bytes smaller (the
    // width of the `/Filter /FlateDecode` it would have to add). Measured
    // 2026-09-23, that guard is what keeps `many-pages.pdf` — 50 tiny content
    // streams — from growing by 2.2%: it deflates none of them, and deflates
    // all six of `unicode-text.pdf`'s for a 92.8% saving.
    for object in doc.objects.values_mut() {
        if let Object::Stream(stream) = object {
            if stream.allows_compression && stream.dict.get(b"Filter").is_err() {
                let before = stream.content.len();
                if stream.compress().is_ok() && stream.content.len() < before {
                    report.streams_deflated += 1;
                }
            }
        }
    }

    let mut out = Vec::new();
    doc.save_to(&mut out)
        .map_err(|e| CommandError::PdfError(format!("compress: save: {e}")))?;
    report.after_bytes = out.len() as u64;

    // The same rule once more, for the file as a whole. A document that cannot
    // be made smaller is returned untouched rather than returned larger.
    if report.after_bytes >= report.before_bytes {
        return Ok((
            bytes.to_vec(),
            CompressReport {
                after_bytes: report.before_bytes,
                images_recompressed: 0,
                images_untouched: report.images_recompressed + report.images_untouched,
                streams_deflated: 0,
                ..report
            },
        ));
    }
    Ok((out, report))
}

/// SPEC: P7-OCR-010 — compress the open document at `level` and write the
/// result to `dest`.
///
/// **Read-only on the open document.** Compression is lossy, so it writes a new
/// file rather than becoming an undoable edit: an edit would let the next
/// Ctrl-S overwrite the user's original with the degraded version, and the undo
/// that could have rescued it dies with the session. The same shape as
/// `extract_pages`.
///
/// # Errors
/// When the document cannot be serialized, compressed, or written.
pub fn compress_document(
    doc: &pdfium_render::prelude::PdfDocument<'_>,
    level: CompressLevel,
    dest: &std::path::Path,
) -> Result<CompressReport, CommandError> {
    let bytes = {
        let _guard = crate::pdf::document::pdfium_lock()?;
        doc.save_to_bytes().map_err(CommandError::from)?
    };
    let (out, report) = compress_bytes(&bytes, level)?;
    std::fs::write(dest, &out)?;
    Ok(report)
}

#[allow(clippy::expect_used, clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::Dictionary;

    /// A minimal image dictionary, which each test then spoils in one way.
    fn image_dict() -> Dictionary {
        let mut d = Dictionary::new();
        d.set("Subtype", Object::Name(b"Image".to_vec()));
        d.set("Width", 800_i64);
        d.set("Height", 600_i64);
        d.set("BitsPerComponent", 8_i64);
        d.set("ColorSpace", Object::Name(b"DeviceGray".to_vec()));
        d
    }

    #[test]
    fn the_levels_trade_resolution_for_size_in_order() {
        assert_eq!(CompressLevel::Low.target_dpi(), None);
        assert_eq!(CompressLevel::Medium.target_dpi(), Some(200.0));
        assert_eq!(CompressLevel::High.target_dpi(), Some(150.0));
        // Quality never rises as the level gets more aggressive.
        assert!(CompressLevel::Low.jpeg_quality() >= CompressLevel::Medium.jpeg_quality());
        assert!(CompressLevel::Medium.jpeg_quality() >= CompressLevel::High.jpeg_quality());
    }

    #[test]
    fn a_plain_grayscale_image_is_eligible() {
        let doc = Document::new();
        assert_eq!(eligibility(&doc, &image_dict()), Ok(Samples::Gray));
    }

    #[test]
    fn device_rgb_is_three_channels() {
        let doc = Document::new();
        let mut d = image_dict();
        d.set("ColorSpace", Object::Name(b"DeviceRGB".to_vec()));
        assert_eq!(eligibility(&doc, &d), Ok(Samples::Rgb));
    }

    #[test]
    fn a_bilevel_image_is_refused() {
        // 1 bit a sample is a CCITT or JBIG2 scan. JPEG would fringe it.
        let doc = Document::new();
        let mut d = image_dict();
        d.set("BitsPerComponent", 1_i64);
        assert_eq!(eligibility(&doc, &d), Err(Ineligible::Bilevel));
    }

    #[test]
    fn sixteen_bit_samples_are_refused() {
        let doc = Document::new();
        let mut d = image_dict();
        d.set("BitsPerComponent", 16_i64);
        assert_eq!(eligibility(&doc, &d), Err(Ineligible::BitDepth));
    }

    #[test]
    fn jpeg_2000_is_refused() {
        let doc = Document::new();
        let mut d = image_dict();
        d.set("Filter", Object::Name(b"JPXDecode".to_vec()));
        assert_eq!(eligibility(&doc, &d), Err(Ineligible::Jpeg2000));
        // Also when it sits in a filter chain rather than alone.
        let mut d = image_dict();
        d.set(
            "Filter",
            Object::Array(vec![
                Object::Name(b"FlateDecode".to_vec()),
                Object::Name(b"JPXDecode".to_vec()),
            ]),
        );
        assert_eq!(eligibility(&doc, &d), Err(Ineligible::Jpeg2000));
    }

    #[test]
    fn a_masked_image_is_refused() {
        // Colour-key masking picks pixels by exact value, and JPEG changes
        // values — the mask would start hiding the wrong pixels.
        let doc = Document::new();
        let mut d = image_dict();
        d.set("Mask", Object::Array(vec![Object::Integer(0), Object::Integer(8)]));
        assert_eq!(eligibility(&doc, &d), Err(Ineligible::Masked));
    }

    #[test]
    fn a_soft_mask_is_not_a_reason_to_refuse() {
        // /SMask is a separate image. The base can be re-encoded; the mask is
        // simply left alone. Refusing here would exclude most transparent PNGs.
        let doc = Document::new();
        let mut d = image_dict();
        d.set("SMask", Object::Reference((9, 0)));
        assert_eq!(eligibility(&doc, &d), Ok(Samples::Gray));
    }

    #[test]
    fn indexed_and_separation_spaces_are_refused() {
        // A sample here is a palette index or an ink quantity, not a colour: a
        // lossy re-encode would change *which* colour is named.
        let doc = Document::new();
        for space in [b"Indexed".to_vec(), b"Separation".to_vec(), b"DeviceN".to_vec()] {
            let mut d = image_dict();
            d.set("ColorSpace", Object::Array(vec![Object::Name(space)]));
            assert_eq!(eligibility(&doc, &d), Err(Ineligible::ColourSpace));
        }
        // And CMYK, which we could encode but could get subtly inverted.
        let mut d = image_dict();
        d.set("ColorSpace", Object::Name(b"DeviceCMYK".to_vec()));
        assert_eq!(eligibility(&doc, &d), Err(Ineligible::ColourSpace));
    }

    #[test]
    fn a_tiny_image_is_not_worth_re_encoding() {
        let doc = Document::new();
        let mut d = image_dict();
        d.set("Width", 16_i64);
        d.set("Height", 16_i64);
        assert_eq!(eligibility(&doc, &d), Err(Ineligible::TooSmall));
    }

    #[test]
    fn a_form_xobject_is_not_an_image() {
        let doc = Document::new();
        let mut d = image_dict();
        d.set("Subtype", Object::Name(b"Form".to_vec()));
        assert_eq!(eligibility(&doc, &d), Err(Ineligible::NotAnImage));
    }

    #[test]
    fn downsampling_only_happens_above_the_target() {
        // A US-Letter page is 612 pt = 8.5 in. At 300 DPI that is 2550 px.
        let letter = 612.0;
        assert_eq!(downsampled_width(2550, letter, 200.0), Some(1700));
        assert_eq!(downsampled_width(2550, letter, 150.0), Some(1275));
        // Already at or below the target: left alone, never upscaled.
        assert_eq!(downsampled_width(1275, letter, 150.0), None);
        assert_eq!(downsampled_width(600, letter, 150.0), None);
        // A page with no width tells us nothing, so nothing is done.
        assert_eq!(downsampled_width(2550, 0.0, 150.0), None);
    }

    #[test]
    fn the_dpi_estimate_errs_towards_keeping_pixels() {
        // The estimate uses the *page* width, not the width the image is drawn
        // at. A 2000 px logo placed one inch wide on a Letter page therefore
        // reads as ~235 DPI rather than 2000, and survives a 200 DPI target
        // almost intact instead of being downsampled to a blur. Under-shrinking
        // is the error we choose.
        let letter = 612.0;
        assert_eq!(downsampled_width(2000, letter, 200.0), Some(1700));
        assert_eq!(downsampled_width(2000, letter, 300.0), None);
    }

    #[test]
    fn a_report_never_claims_a_negative_saving() {
        let report = CompressReport {
            before_bytes: 100,
            after_bytes: 140,
            ..CompressReport::default()
        };
        assert_eq!(report.saved_bytes(), 0);
    }
}
