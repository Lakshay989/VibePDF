//! SPEC: P7-OCR-005 — export pages as images: PNG, JPG, TIFF or WebP, at a
//! DPI the user chooses between 72 and 600.
//!
//! The rasterisation itself is not here. [`crate::pdf::render::render_page`]
//! already owns the DPI arithmetic, the page-index check and the
//! process-global `PDFium` lock, so this module asks it for RGBA pixels and
//! does the part that is new: choosing an encoder, flattening alpha where the
//! format has none, naming the files and writing them.
//!
//! **One file per page, named by the page's own number.** Exporting page 5 on
//! its own writes `report-005.png`, not `report-001.png` — an image file *is*
//! page 5, unlike a split file, which is a new document that starts at its own
//! page 1 (`split.rs` numbers by sequence for exactly that reason).
//!
//! **Memory.** A US-Letter page at 600 DPI is 5100 × 6600 × 4 ≈ 135 MB of
//! RGBA. Each page is rendered, encoded, written and dropped before the next
//! one starts, so a 50-page run costs one page's worth of memory, not fifty.

use std::io::Cursor;
use std::path::Path;

use image::codecs::jpeg::JpegEncoder;
use image::codecs::webp::WebPEncoder;
use image::{ExtendedColorType, ImageEncoder};
use pdfium_render::prelude::PdfDocument;
use serde::{Deserialize, Serialize};
use tiff::encoder::{colortype, Compression, DeflateLevel, TiffEncoder};

use crate::error::CommandError;
use crate::pdf::document::pdfium_lock;
use crate::pdf::render::{encode_png, render_page, ImageFormat};

/// The four formats P7-OCR-005 names.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageExportFormat {
    /// Lossless, alpha kept, the safe default.
    Png,
    /// Lossy. JPEG has three channels, so the RGBA render is composited down
    /// to RGB first — see [`flatten_over_white`].
    Jpeg,
    /// Lossless, alpha kept, Deflate-compressed. The archival choice.
    Tiff,
    /// Lossless only. `image` 0.25's encoder is VP8L; lossy WebP needs the C
    /// `libwebp`, which we do not link. Smaller than PNG, larger than a lossy
    /// WebP would be.
    Webp,
}

impl ImageExportFormat {
    /// The file extension, without the dot.
    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Tiff => "tiff",
            Self::Webp => "webp",
        }
    }

    /// Parse the wire value. The frontend sends the serde name.
    pub fn parse(name: &str) -> Result<Self, CommandError> {
        match name {
            "png" => Ok(Self::Png),
            "jpeg" | "jpg" => Ok(Self::Jpeg),
            "tiff" | "tif" => Ok(Self::Tiff),
            "webp" => Ok(Self::Webp),
            other => Err(CommandError::InvalidInput(format!(
                "unknown image format: {other}"
            ))),
        }
    }
}

/// The low end of P7-OCR-005's range: screen resolution.
pub const MIN_DPI: f32 = 72.0;
/// The high end: print resolution. Above this a page stops being an image and
/// becomes a memory problem.
pub const MAX_DPI: f32 = 600.0;

/// What to export and how.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageExportOptions {
    pub format: ImageExportFormat,
    /// Dots per inch, `72..=600`. Outside that range is rejected rather than
    /// clamped: the spec names the range, and silently changing what the
    /// caller asked for hides a bug on their side.
    pub dpi: f32,
    /// JPEG quality, `1..=100`. Ignored by the three lossless formats.
    pub quality: u8,
}

impl Default for ImageExportOptions {
    fn default() -> Self {
        Self {
            format: ImageExportFormat::Png,
            // 150 DPI: twice screen, half print. Legible when zoomed, and a
            // 50-page export still fits in a mail attachment.
            dpi: 150.0,
            // 85 is the usual "no visible artefacts at 100%" setting.
            quality: 85,
        }
    }
}

/// What an export wrote, for the caller and the UI.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageExportSummary {
    pub pages: u32,
    /// File names only, in the order written. The caller knows the directory.
    pub files: Vec<String>,
    pub bytes: u64,
}

/// SPEC: P7-OCR-005 — check the options before a single page is rendered, so a
/// bad DPI costs nothing.
fn validate(options: ImageExportOptions) -> Result<(), CommandError> {
    if !options.dpi.is_finite() || options.dpi < MIN_DPI || options.dpi > MAX_DPI {
        return Err(CommandError::InvalidInput(format!(
            "dpi {} is outside the supported range {}–{}",
            options.dpi, MIN_DPI, MAX_DPI
        )));
    }
    if options.format == ImageExportFormat::Jpeg && !(1..=100).contains(&options.quality) {
        return Err(CommandError::InvalidInput(format!(
            "jpeg quality {} is outside 1–100",
            options.quality
        )));
    }
    Ok(())
}

/// A file stem that would escape the chosen directory, or name nothing, is a
/// caller bug — the UI derives it from the document's own file name.
fn check_stem(stem: &str) -> Result<(), CommandError> {
    if stem.is_empty() {
        return Err(CommandError::InvalidInput("empty file name".into()));
    }
    if stem.contains('/') || stem.contains('\\') || stem.contains("..") {
        return Err(CommandError::InvalidInput(format!(
            "file name must not contain a path: {stem}"
        )));
    }
    Ok(())
}

/// `{stem}-{page:03}.{ext}`, where `page` is 1-based — the number the reader
/// sees in the page thumbnail, not the loop counter.
#[must_use]
pub fn file_name(stem: &str, page_number: usize, format: ImageExportFormat) -> String {
    format!("{stem}-{page_number:03}.{}", format.extension())
}

/// Composite RGBA down to RGB over an opaque white page, for JPEG, which has
/// no fourth channel to put alpha in.
///
/// Measured 2026-09-23: `PDFium` renders pages fully opaque — every one of the
/// 484,704 pixels of `hello.pdf` at 72 DPI came back with `a == 255`. So the
/// *conversion* is what JPEG needs; compositing over **white** rather than
/// dropping the channel is insurance against a future `PDFium` (or a caller
/// that sets a clear colour) handing us real transparency, where dropping
/// alpha would silently paint the transparent parts black.
///
/// Straight alpha, `out = src * a + white * (1 - a)`, in 8-bit.
#[must_use]
pub fn flatten_over_white(rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len() / 4 * 3);
    for px in rgba.chunks_exact(4) {
        let a = u32::from(px[3]);
        for &channel in &px[..3] {
            // 255 * (255 - a) is the white contribution; + 127 rounds.
            let v = (u32::from(channel) * a + 255 * (255 - a) + 127) / 255;
            #[allow(clippy::cast_possible_truncation)]
            out.push(v.min(255) as u8);
        }
    }
    out
}

/// Encode one rendered page. Returns the bytes to write.
fn encode(
    width: u32,
    height: u32,
    rgba: &[u8],
    options: ImageExportOptions,
) -> Result<Vec<u8>, CommandError> {
    if width == 0 || height == 0 {
        return Err(CommandError::Internal(
            "cannot encode zero-dimension bitmap".into(),
        ));
    }
    match options.format {
        ImageExportFormat::Png => encode_png(width, height, rgba),
        ImageExportFormat::Jpeg => {
            let rgb = flatten_over_white(rgba);
            let mut out = Vec::new();
            JpegEncoder::new_with_quality(Cursor::new(&mut out), options.quality)
                .write_image(&rgb, width, height, ExtendedColorType::Rgb8)
                .map_err(|e| CommandError::Internal(format!("jpeg encode: {e}")))?;
            Ok(out)
        }
        ImageExportFormat::Tiff => {
            // The `tiff` crate rather than `image`'s wrapper around it: the
            // wrapper writes every page uncompressed (8.4 MB for one US-Letter
            // page at 150 DPI, measured 2026-09-23) and offers no way to say
            // otherwise. Deflate is the lossless TIFF compression every reader
            // handles; the pixels are identical either way.
            let mut out = Vec::new();
            TiffEncoder::new(Cursor::new(&mut out))
                .map_err(|e| CommandError::Internal(format!("tiff header: {e}")))?
                .with_compression(Compression::Deflate(DeflateLevel::Balanced))
                .write_image::<colortype::RGBA8>(width, height, rgba)
                .map_err(|e| CommandError::Internal(format!("tiff encode: {e}")))?;
            Ok(out)
        }
        ImageExportFormat::Webp => {
            let mut out = Vec::new();
            WebPEncoder::new_lossless(Cursor::new(&mut out))
                .write_image(rgba, width, height, ExtendedColorType::Rgba8)
                .map_err(|e| CommandError::Internal(format!("webp encode: {e}")))?;
            Ok(out)
        }
    }
}

/// SPEC: P7-OCR-005 — render each of `pages` (0-based, empty meaning the whole
/// document) at `options.dpi` and write it into `dest_dir` as
/// `{stem}-{n:03}.{ext}`.
///
/// The PDF is only read; nothing here modifies the document, so the caller has
/// no edit to undo.
///
/// # Errors
/// When the options are out of range, the stem names a path, `dest_dir` is not
/// a directory, a page index is past the end, or a render, encode or write
/// fails.
pub fn export_pages(
    doc: &PdfDocument<'_>,
    pages: &[usize],
    dest_dir: &Path,
    stem: &str,
    options: ImageExportOptions,
) -> Result<ImageExportSummary, CommandError> {
    validate(options)?;
    check_stem(stem)?;
    if !dest_dir.is_dir() {
        return Err(CommandError::InvalidInput(format!(
            "not a directory: {}",
            dest_dir.display()
        )));
    }

    // An empty selection means the whole document, as it does for text export.
    // The lock is taken only to count the pages and released before the first
    // render — `render_page` takes it itself, once per page, and it is not
    // reentrant.
    let wanted: Vec<usize> = if pages.is_empty() {
        let count = {
            let _guard = pdfium_lock()?;
            doc.pages().len()
        };
        let count = usize::try_from(count)
            .map_err(|_| CommandError::Internal(format!("negative page count: {count}")))?;
        (0..count).collect()
    } else {
        pages.to_vec()
    };
    if wanted.is_empty() {
        return Err(CommandError::InvalidInput("no pages selected".into()));
    }

    let mut files = Vec::with_capacity(wanted.len());
    let mut bytes: u64 = 0;
    for &page in &wanted {
        let index = u32::try_from(page)
            .map_err(|_| CommandError::InvalidInput(format!("page {page} out of range")))?;
        // Rgba8 rather than Png: this module owns the encoder choice, and for
        // PNG `encode` calls the same `encode_png` render.rs would have.
        let rendered = render_page(doc, index, options.dpi, ImageFormat::Rgba8)?;
        let encoded = encode(rendered.width, rendered.height, &rendered.bytes, options)?;
        // Dropping the render before the write keeps one page in memory, not
        // two — at 600 DPI that is the difference that matters.
        drop(rendered);
        let name = file_name(stem, page + 1, options.format);
        std::fs::write(dest_dir.join(&name), &encoded)?;
        bytes += encoded.len() as u64;
        files.push(name);
    }

    Ok(ImageExportSummary {
        pages: u32::try_from(files.len()).unwrap_or(u32::MAX),
        files,
        bytes,
    })
}

#[allow(clippy::expect_used, clippy::unwrap_used)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_files_by_the_pages_own_number() {
        assert_eq!(
            file_name("report", 5, ImageExportFormat::Png),
            "report-005.png"
        );
        assert_eq!(
            file_name("report", 142, ImageExportFormat::Jpeg),
            "report-142.jpg"
        );
        // Past three digits the number simply grows; it is not truncated.
        assert_eq!(
            file_name("report", 1000, ImageExportFormat::Tiff),
            "report-1000.tiff"
        );
    }

    #[test]
    fn each_format_has_its_own_extension() {
        assert_eq!(ImageExportFormat::Png.extension(), "png");
        assert_eq!(ImageExportFormat::Jpeg.extension(), "jpg");
        assert_eq!(ImageExportFormat::Tiff.extension(), "tiff");
        assert_eq!(ImageExportFormat::Webp.extension(), "webp");
    }

    #[test]
    fn parses_the_wire_names_and_rejects_the_rest() {
        let parsed = |name: &str| ImageExportFormat::parse(name).expect("known format");
        assert_eq!(parsed("png"), ImageExportFormat::Png);
        assert_eq!(parsed("jpg"), ImageExportFormat::Jpeg);
        assert_eq!(parsed("jpeg"), ImageExportFormat::Jpeg);
        assert_eq!(parsed("tif"), ImageExportFormat::Tiff);
        assert_eq!(parsed("tiff"), ImageExportFormat::Tiff);
        assert_eq!(parsed("webp"), ImageExportFormat::Webp);
        assert!(ImageExportFormat::parse("gif").is_err());
        // Case matters: the frontend sends the serde name, lower-case.
        assert!(ImageExportFormat::parse("PNG").is_err());
    }

    #[test]
    fn dpi_outside_the_spec_range_is_rejected() {
        let at = |dpi: f32| {
            validate(ImageExportOptions {
                dpi,
                ..ImageExportOptions::default()
            })
        };
        // SPEC: P7-OCR-005 names 72–600. Both ends are inclusive.
        assert!(at(72.0).is_ok());
        assert!(at(600.0).is_ok());
        assert!(at(71.9).is_err());
        assert!(at(600.1).is_err());
        assert!(at(f32::NAN).is_err());
        assert!(at(f32::INFINITY).is_err());
        assert!(at(-300.0).is_err());
    }

    #[test]
    fn jpeg_quality_is_bounded_but_only_for_jpeg() {
        let with = |format, quality| {
            validate(ImageExportOptions {
                format,
                quality,
                ..ImageExportOptions::default()
            })
        };
        assert!(with(ImageExportFormat::Jpeg, 1).is_ok());
        assert!(with(ImageExportFormat::Jpeg, 100).is_ok());
        assert!(with(ImageExportFormat::Jpeg, 0).is_err());
        // The lossless formats ignore quality, so a nonsense value is not an
        // error there — it is simply unused.
        assert!(with(ImageExportFormat::Png, 0).is_ok());
        assert!(with(ImageExportFormat::Webp, 0).is_ok());
    }

    #[test]
    fn a_stem_that_names_a_path_is_rejected() {
        assert!(check_stem("report").is_ok());
        assert!(check_stem("my report (final)").is_ok());
        assert!(check_stem("").is_err());
        assert!(check_stem("../escape").is_err());
        assert!(check_stem("sub/dir").is_err());
        assert!(check_stem("sub\\dir").is_err());
    }

    #[test]
    fn flattening_composites_over_white() {
        // Opaque pixels pass through untouched.
        assert_eq!(flatten_over_white(&[10, 20, 30, 255]), vec![10, 20, 30]);
        // A fully transparent pixel becomes white whatever its colour bytes
        // say — this is the case that writes a black page if it is skipped.
        assert_eq!(flatten_over_white(&[0, 0, 0, 0]), vec![255, 255, 255]);
        // Half-transparent black over white is mid grey.
        let half = flatten_over_white(&[0, 0, 0, 128]);
        assert!(
            (126..=129).contains(&half[0]),
            "expected mid grey, got {half:?}"
        );
        // Length is 3 bytes per pixel, not 4.
        assert_eq!(flatten_over_white(&[1, 2, 3, 255, 4, 5, 6, 255]).len(), 6);
    }

    #[test]
    fn every_format_encodes_to_its_own_magic_bytes() {
        // A 2x2 opaque red square.
        let rgba = [255u8, 0, 0, 255].repeat(4);
        let bytes = |format| {
            encode(
                2,
                2,
                &rgba,
                ImageExportOptions {
                    format,
                    ..ImageExportOptions::default()
                },
            )
            .expect("encode")
        };
        assert_eq!(&bytes(ImageExportFormat::Png)[..4], b"\x89PNG");
        assert_eq!(&bytes(ImageExportFormat::Jpeg)[..2], b"\xff\xd8");
        let tiff = bytes(ImageExportFormat::Tiff);
        assert!(
            &tiff[..2] == b"II" || &tiff[..2] == b"MM",
            "not a TIFF byte-order mark: {:?}",
            &tiff[..2]
        );
        let webp = bytes(ImageExportFormat::Webp);
        assert_eq!(&webp[..4], b"RIFF");
        assert_eq!(&webp[8..12], b"WEBP");
    }

    #[test]
    fn a_zero_dimension_bitmap_is_an_error_not_a_panic() {
        assert!(encode(0, 0, &[], ImageExportOptions::default()).is_err());
    }
}
