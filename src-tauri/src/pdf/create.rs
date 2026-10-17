//! SPEC: P7-OCR-009 (P7.C1a) — create a PDF from image and plain-text files.
//!
//! The document is built object by object with lopdf, the way the header,
//! footer and watermark text already is, and then goes through the verified
//! save path like every other write: loaded into `PDFium`, written to a
//! temporary file, reopened, renamed into place.
//!
//! **Images.** One per page, scaled down to fit inside the margins (never up:
//! a small image blown up to fill a page is only blur), centred, the page
//! turned landscape when the image is wider than tall. A JPEG is copied in
//! byte for byte as a `DCTDecode` stream — re-compressing it would lose quality
//! for nothing — and a phone photo's EXIF orientation is applied by the
//! placement matrix rather than by decoding it. PNG, TIFF and WebP are decoded
//! and stored losslessly, with their transparency as a soft mask.
//!
//! **Text.** Laid out in an embedded font, wrapped at the margin, a new page
//! when one fills and at every form feed. The font is a monospaced system
//! face when one covers every character — plain text lines things up with
//! spaces — and otherwise the broad-coverage face the app already uses for
//! Unicode. A character no font on the computer can draw is refused by name
//! rather than drawn as an empty box.

// Pixel and glyph counts become PDF coordinates (f32). They are bounded —
// images by `MAX_PIXELS`, text by the page — so the precision loss in those
// casts is far below a printer's dot.
#![allow(clippy::cast_precision_loss)]

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use serde::{Deserialize, Serialize};

use crate::error::CommandError;
use crate::pdf::document::{load_edited_bytes, pdfium_lock, save_document};
use crate::pdf::font_embed_cid::{build_cid_font, CidFont};
use crate::pdf::font_resolver::broad_coverage_font_paths;

/// The page every image or text page is placed on.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum PageSize {
    A4,
    Letter,
    /// The page is the image's own size, one pixel to a point. Text files
    /// still go on A4.
    Image,
}

impl PageSize {
    /// Portrait width and height in points; `None` for [`PageSize::Image`].
    fn portrait(self) -> Option<(f32, f32)> {
        match self {
            Self::A4 => Some((595.28, 841.89)),
            Self::Letter => Some((612.0, 792.0)),
            Self::Image => None,
        }
    }
}

/// What a creation produced.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateSummary {
    pub pages: u32,
    pub images: u32,
    pub text_files: u32,
    pub bytes: u64,
    /// Things done that the person should know about, such as a text file
    /// read as Windows-1252 because it was not UTF-8.
    pub warnings: Vec<String>,
}

/// Space kept clear around an image on an A4 or Letter page.
const IMAGE_MARGIN: f32 = 36.0;
/// Space around text: 0.75 in, which fits an 80-column line at 10 pt on A4.
const TEXT_MARGIN: f32 = 54.0;
const TEXT_SIZE: f32 = 10.0;
const LEADING: f32 = 12.0;
const TAB_STOP: usize = 8;
/// Decoding a 100-megapixel image takes 300–400 MB. Anything larger is
/// refused rather than allowed to exhaust memory.
const MAX_PIXELS: u64 = 100_000_000;
const TEXT_EXTENSIONS: [&str; 5] = ["txt", "text", "log", "md", "csv"];

/// A PDF being assembled: the objects, and the pages in order.
struct Builder {
    doc: Document,
    pages_id: ObjectId,
    kids: Vec<ObjectId>,
}

impl Builder {
    fn new() -> Self {
        let mut doc = Document::with_version("1.7");
        let pages_id = doc.new_object_id();
        Self { doc, pages_id, kids: Vec::new() }
    }

    fn add_page(&mut self, (width, height): (f32, f32), content: &str, resources: Dictionary) {
        let mut stream = Stream::new(Dictionary::new(), content.as_bytes().to_vec());
        // A content stream that would not shrink is simply stored.
        let _ = stream.compress();
        let content_id = self.doc.add_object(stream);
        let page = dictionary! {
            "Type" => "Page",
            "Parent" => self.pages_id,
            "MediaBox" => vec![0.into(), 0.into(), Object::Real(width), Object::Real(height)],
            "Contents" => content_id,
            "Resources" => resources,
        };
        self.kids.push(self.doc.add_object(page));
    }

    fn finish(mut self) -> Result<Vec<u8>, CommandError> {
        let count = i64::try_from(self.kids.len()).unwrap_or(i64::MAX);
        let kids: Vec<Object> = self.kids.iter().map(|id| Object::Reference(*id)).collect();
        self.doc.objects.insert(
            self.pages_id,
            Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => count }),
        );
        let catalog = self.doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => self.pages_id });
        self.doc.trailer.set("Root", catalog);
        let info = self.doc.add_object(dictionary! { "Producer" => Object::string_literal("VibePDF") });
        self.doc.trailer.set("Info", info);
        let mut out = Vec::new();
        self.doc
            .save_to(&mut out)
            .map_err(|e| CommandError::PdfError(format!("could not write the new PDF: {e}")))?;
        Ok(out)
    }
}

/// A number for a content stream: at most three decimals, no trailing zeros.
fn num(value: f32) -> String {
    let text = format!("{value:.3}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" { "0".into() } else { text.into() }
}

fn file_label(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

// ---------------------------------------------------------------------------
// Images

/// What a JPEG's own header says, read without decoding it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct JpegInfo {
    width: u32,
    height: u32,
    components: u8,
    precision: u8,
    /// The start-of-frame marker: `0xC0` baseline, `0xC1` extended, `0xC2`
    /// progressive, and so on.
    frame: u8,
    /// An Adobe `APP14` segment. A four-component JPEG carrying one stores its
    /// CMYK values inverted, which `/Decode` has to undo.
    adobe: bool,
}

/// Walk a JPEG's markers up to its frame header.
fn jpeg_info(bytes: &[u8]) -> Option<JpegInfo> {
    if bytes.get(..2)? != [0xFF, 0xD8] {
        return None;
    }
    let mut at = 2;
    let mut adobe = false;
    while at + 4 <= bytes.len() {
        if bytes[at] != 0xFF {
            return None;
        }
        let marker = bytes[at + 1];
        if marker == 0xFF {
            at += 1; // fill byte
            continue;
        }
        if marker == 0x01 || (0xD0..=0xD8).contains(&marker) {
            at += 2; // markers with no length
            continue;
        }
        let length = usize::from(u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]));
        if length < 2 {
            return None;
        }
        let segment = bytes.get(at + 4..at + 2 + length)?;
        match marker {
            0xEE if segment.starts_with(b"Adobe") => adobe = true,
            // Every SOFn, minus the three markers that share the range.
            0xC0..=0xCF if !matches!(marker, 0xC4 | 0xC8 | 0xCC) => {
                let header = segment.get(..6)?;
                return Some(JpegInfo {
                    precision: header[0],
                    height: u32::from(u16::from_be_bytes([header[1], header[2]])),
                    width: u32::from(u16::from_be_bytes([header[3], header[4]])),
                    components: header[5],
                    frame: marker,
                    adobe,
                });
            }
            0xDA => return None, // scan data before any frame header
            _ => {}
        }
        at += 2 + length;
    }
    None
}

/// Whether a JPEG can go into the PDF as it is: eight-bit, Huffman-coded
/// (baseline, extended or progressive — what `DCTDecode` reads everywhere),
/// and one, three or four components.
fn passes_through(info: &JpegInfo) -> bool {
    info.precision == 8
        && matches!(info.frame, 0xC0..=0xC2)
        && matches!(info.components, 1 | 3 | 4)
        && info.width > 0
        && info.height > 0
}

/// Whether showing an image in `orientation` swaps its width and height.
fn turns_sideways(orientation: Orientation) -> bool {
    matches!(
        orientation,
        Orientation::Rotate90 | Orientation::Rotate270 | Orientation::Rotate90FlipH | Orientation::Rotate270FlipH
    )
}

/// The `cm` matrix that draws an image's unit square into the box at `(x, y)`,
/// `width` × `height` as *shown*, turned and flipped the way `orientation`
/// says. The stored image's first row is the top of the unit square.
///
/// Each case is where the stored image's corners land: for `Rotate90`
/// (clockwise), its top-left corner goes to the box's top-right.
fn placement(orientation: Orientation, x: f32, y: f32, width: f32, height: f32) -> [f32; 6] {
    match orientation {
        Orientation::NoTransforms => [width, 0.0, 0.0, height, x, y],
        Orientation::Rotate180 => [-width, 0.0, 0.0, -height, x + width, y + height],
        Orientation::FlipHorizontal => [-width, 0.0, 0.0, height, x + width, y],
        Orientation::FlipVertical => [width, 0.0, 0.0, -height, x, y + height],
        Orientation::Rotate90 => [0.0, -height, width, 0.0, x, y + height],
        Orientation::Rotate270 => [0.0, height, -width, 0.0, x + width, y],
        Orientation::Rotate90FlipH => [0.0, -height, -width, 0.0, x + width, y + height],
        Orientation::Rotate270FlipH => [0.0, height, width, 0.0, x, y],
    }
}

/// The page for an image `shown` (width, height) in pixels, and the box it is
/// drawn into.
fn image_page(size: PageSize, (shown_w, shown_h): (f32, f32)) -> ((f32, f32), [f32; 4]) {
    let Some((short, long)) = size.portrait() else {
        return ((shown_w, shown_h), [0.0, 0.0, shown_w, shown_h]);
    };
    let page = if shown_w > shown_h { (long, short) } else { (short, long) };
    let room = (page.0 - 2.0 * IMAGE_MARGIN, page.1 - 2.0 * IMAGE_MARGIN);
    let scale = (room.0 / shown_w).min(room.1 / shown_h).min(1.0);
    let (w, h) = (shown_w * scale, shown_h * scale);
    (page, [(page.0 - w) / 2.0, (page.1 - h) / 2.0, w, h])
}

/// An image `XObject` for pixels that were decoded, with a soft mask when any
/// of them is not fully opaque.
fn decoded_xobject(doc: &mut Document, width: u32, height: u32, colour: &[u8], gray: bool, alpha: Option<&[u8]>) -> ObjectId {
    let image_dict = |space: &str| {
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => i64::from(width),
            "Height" => i64::from(height),
            "ColorSpace" => space,
            "BitsPerComponent" => 8,
        }
    };
    let mut dict = image_dict(if gray { "DeviceGray" } else { "DeviceRGB" });
    if let Some(alpha) = alpha.filter(|a| a.iter().any(|&v| v != 255)) {
        let mut mask = Stream::new(image_dict("DeviceGray"), alpha.to_vec());
        let _ = mask.compress();
        dict.set("SMask", doc.add_object(mask));
    }
    let mut stream = Stream::new(dict, colour.to_vec());
    let _ = stream.compress();
    doc.add_object(stream)
}

/// Pixels ready for an image `XObject`: eight-bit gray or RGB, plus an alpha
/// channel when the source had one.
struct Decoded {
    width: u32,
    height: u32,
    colour: Vec<u8>,
    gray: bool,
    alpha: Option<Vec<u8>>,
}

/// A PNG, decoded by the `png` crate (which this project already ships).
fn decode_png(name: &str, bytes: &[u8]) -> Result<Decoded, CommandError> {
    let unreadable = |e: &dyn std::fmt::Display| CommandError::InvalidInput(format!("{name} could not be read as a PNG: {e}"));
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| unreadable(&e))?;
    let (width, height) = (reader.info().width, reader.info().height);
    refuse_if_too_large(name, width, height)?;
    let mut buffer = vec![0; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut buffer).map_err(|e| unreadable(&e))?;
    let pixels = &buffer[..frame.buffer_size()];
    let (colour, gray, alpha) = match frame.color_type {
        png::ColorType::Grayscale => (pixels.to_vec(), true, None),
        png::ColorType::GrayscaleAlpha => {
            (pixels.iter().step_by(2).copied().collect(), true, Some(pixels.iter().skip(1).step_by(2).copied().collect()))
        }
        png::ColorType::Rgb => (pixels.to_vec(), false, None),
        png::ColorType::Rgba => (
            pixels.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect(),
            false,
            Some(pixels.chunks_exact(4).map(|p| p[3]).collect()),
        ),
        png::ColorType::Indexed => return Err(unreadable(&"its palette could not be expanded")),
    };
    Ok(Decoded { width, height, colour, gray, alpha })
}

fn refuse_if_too_large(name: &str, width: u32, height: u32) -> Result<(), CommandError> {
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(CommandError::InvalidInput(format!(
            "{name} is {width} × {height} pixels, over the 100-megapixel limit for a page image."
        )));
    }
    Ok(())
}

/// Add one image as one page.
fn add_image(builder: &mut Builder, name: &str, bytes: &[u8], format: ImageFormat, size: PageSize) -> Result<(), CommandError> {
    let unreadable = |e: &dyn std::fmt::Display| CommandError::InvalidInput(format!("{name} could not be read as an image: {e}"));

    // JPEG: copied in unchanged; its orientation goes into the matrix.
    if format == ImageFormat::Jpeg {
        if let Some(info) = jpeg_info(bytes).filter(passes_through) {
            let orientation = image::codecs::jpeg::JpegDecoder::new(Cursor::new(bytes))
                .and_then(|mut d| d.orientation())
                .unwrap_or(Orientation::NoTransforms);
            let (w, h) = (info.width as f32, info.height as f32);
            let shown = if turns_sideways(orientation) { (h, w) } else { (w, h) };
            let (page, [x, y, bw, bh]) = image_page(size, shown);

            let space = match info.components {
                1 => "DeviceGray",
                3 => "DeviceRGB",
                _ => "DeviceCMYK",
            };
            let mut dict = dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => i64::from(info.width),
                "Height" => i64::from(info.height),
                "ColorSpace" => space,
                "BitsPerComponent" => 8,
                "Filter" => "DCTDecode",
            };
            if info.components == 4 && info.adobe {
                dict.set("Decode", (0..4).flat_map(|_| [Object::Integer(1), Object::Integer(0)]).collect::<Vec<_>>());
            }
            let xobject = builder.doc.add_object(Stream::new(dict, bytes.to_vec()));
            draw_image(builder, page, xobject, placement(orientation, x, y, bw, bh));
            return Ok(());
        }
    }

    // Everything else is decoded, turned upright, and stored losslessly.
    let Decoded { width, height, colour, gray, alpha } = if format == ImageFormat::Png {
        decode_png(name, bytes)?
    } else {
        let mut decoder = ImageReader::with_format(Cursor::new(bytes), format)
            .into_decoder()
            .map_err(|e| unreadable(&e))?;
        let (w, h) = decoder.dimensions();
        refuse_if_too_large(name, w, h)?;
        let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
        let mut image = DynamicImage::from_decoder(decoder).map_err(|e| unreadable(&e))?;
        image.apply_orientation(orientation);
        let gray = !image.color().has_color();
        let alpha = image.color().has_alpha().then(|| image.to_rgba8().pixels().map(|p| p.0[3]).collect::<Vec<u8>>());
        let colour = if gray { image.to_luma8().into_raw() } else { image.to_rgb8().into_raw() };
        Decoded { width: image.width(), height: image.height(), colour, gray, alpha }
    };
    let (page, [x, y, bw, bh]) = image_page(size, (width as f32, height as f32));
    let xobject = decoded_xobject(&mut builder.doc, width, height, &colour, gray, alpha.as_deref());
    draw_image(builder, page, xobject, placement(Orientation::NoTransforms, x, y, bw, bh));
    Ok(())
}

fn draw_image(builder: &mut Builder, page: (f32, f32), xobject: ObjectId, matrix: [f32; 6]) {
    let matrix: Vec<String> = matrix.iter().map(|v| num(*v)).collect();
    let content = format!("q {} cm /Im0 Do Q", matrix.join(" "));
    builder.add_page(page, &content, dictionary! { "XObject" => dictionary! { "Im0" => xobject } });
}

// ---------------------------------------------------------------------------
// Text

/// Decode a text file: UTF-8 or UTF-16 by byte-order mark, UTF-8 without
/// one, and otherwise Windows-1252 — the commonest single-byte encoding —
/// with a warning, since that is a guess.
fn decode_text(name: &str, bytes: &[u8], warnings: &mut Vec<String>) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    let utf16 = |rest: &[u8], big: bool| {
        let units: Vec<u16> = rest
            .chunks_exact(2)
            .map(|p| if big { u16::from_be_bytes([p[0], p[1]]) } else { u16::from_le_bytes([p[0], p[1]]) })
            .collect();
        String::from_utf16_lossy(&units)
    };
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest, false);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(rest, true);
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_owned();
    }
    warnings.push(format!("{name} is not UTF-8, so it was read as Western (Windows-1252)."));
    bytes.iter().map(|&b| windows_1252(b)).collect()
}

/// One Windows-1252 byte. Matches Latin-1 except 0x80–0x9F, where 1252 has
/// printable characters (curly quotes, the euro sign) and Latin-1 has controls.
fn windows_1252(byte: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž', '\u{8F}',
        '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9D}', 'ž', 'Ÿ',
    ];
    match byte {
        0x80..=0x9F => HIGH[usize::from(byte - 0x80)],
        _ => char::from(byte),
    }
}

/// Monospaced faces worth trying first, per platform.
fn monospace_font_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    #[cfg(target_os = "macos")]
    {
        paths.push(PathBuf::from("/System/Library/Fonts/Menlo.ttc"));
        paths.push(PathBuf::from("/System/Library/Fonts/Supplemental/Courier New.ttf"));
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(windir) = std::env::var("SystemRoot") {
            paths.push(PathBuf::from(&windir).join("Fonts").join("consola.ttf"));
            paths.push(PathBuf::from(&windir).join("Fonts").join("cour.ttf"));
        }
    }
    #[cfg(target_os = "linux")]
    {
        paths.push(PathBuf::from("/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"));
        paths.push(PathBuf::from("/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf"));
    }
    paths
}

/// The first face — monospaced ones first — that draws every character of
/// `text`, or the first character none of them can.
fn pick_text_font(name: &str, text: &str) -> Result<Vec<u8>, CommandError> {
    let needed: BTreeSet<char> = text.chars().filter(|c| !c.is_control()).collect();
    let mut faces = Vec::new();
    for path in monospace_font_paths().into_iter().chain(broad_coverage_font_paths()) {
        let Ok(bytes) = std::fs::read(&path) else { continue };
        if ttf_parser::Face::parse(&bytes, 0).is_ok_and(|face| needed.iter().all(|&c| face.glyph_index(c).is_some())) {
            return Ok(bytes);
        }
        faces.push(bytes);
    }
    let uncovered = needed.iter().find(|&&c| {
        !faces.iter().any(|bytes| ttf_parser::Face::parse(bytes, 0).is_ok_and(|face| face.glyph_index(c).is_some()))
    });
    Err(CommandError::InvalidInput(match uncovered {
        Some(c) => format!(
            "{name} uses {c} (U+{:04X}), and no font on this computer can draw it, so the PDF would show an empty box.",
            u32::from(*c)
        ),
        None => format!("{name} uses characters that no single font on this computer covers."),
    }))
}

/// Chinese, Japanese and Korean script: no spaces between words, so a line
/// may break between any two of these characters.
fn breaks_anywhere(c: char) -> bool {
    matches!(u32::from(c),
        0x1100..=0x11FF | 0x2E80..=0x2FDF | 0x3000..=0x303F | 0x3040..=0x30FF | 0x3100..=0x31FF
        | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF
        | 0x20000..=0x2FA1F)
}

/// Tabs to spaces at eight-column stops; any other control character dropped.
fn clean_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut column = 0;
    for c in line.chars() {
        if c == '\t' {
            let spaces = TAB_STOP - column % TAB_STOP;
            out.extend(std::iter::repeat_n(' ', spaces));
            column += spaces;
        } else if !c.is_control() {
            out.push(c);
            column += 1;
        }
    }
    out
}

/// Split one line into pieces that fit `width`: at spaces, between CJK
/// characters, and — for a word longer than the line — between letters.
fn wrap(line: &str, width: f32, measure: &dyn Fn(&str) -> f32) -> Vec<String> {
    if measure(line) <= width {
        return vec![line.to_owned()];
    }
    // Tokens: a run of spaces, a single CJK character, or a run of anything else.
    let mut tokens: Vec<String> = Vec::new();
    for c in line.chars() {
        let joins = tokens.last().is_some_and(|t: &String| {
            let last = t.chars().last().unwrap_or(' ');
            !breaks_anywhere(c) && !breaks_anywhere(last) && (c == ' ') == (last == ' ')
        });
        if joins {
            if let Some(t) = tokens.last_mut() {
                t.push(c);
            }
        } else {
            tokens.push(c.to_string());
        }
    }

    let mut lines = Vec::new();
    let mut current = String::new();
    for token in tokens {
        if measure(&format!("{current}{token}")) <= width {
            current.push_str(&token);
            continue;
        }
        if !current.trim().is_empty() {
            lines.push(std::mem::take(&mut current).trim_end().to_owned());
        }
        current.clear();
        if token.starts_with(' ') {
            continue; // a break swallows the spaces it falls in
        }
        for c in token.chars() {
            if !current.is_empty() && measure(&format!("{current}{c}")) > width {
                lines.push(std::mem::take(&mut current));
            }
            current.push(c);
        }
    }
    if !current.trim().is_empty() {
        lines.push(current.trim_end().to_owned());
    }
    lines
}

/// The text's pages, each a list of lines that fit `width`, `per_page` lines
/// to a page. A form feed starts a new page.
fn lay_out(text: &str, width: f32, per_page: usize, measure: &dyn Fn(&str) -> f32) -> Vec<Vec<String>> {
    let normalised = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut pages = Vec::new();
    for sheet in normalised.split('\u{c}') {
        let sheet = sheet.strip_suffix('\n').unwrap_or(sheet);
        let lines: Vec<String> = sheet.split('\n').flat_map(|raw| {
            let line = clean_line(raw);
            if line.is_empty() { vec![String::new()] } else { wrap(&line, width, measure) }
        }).collect();
        for chunk in lines.chunks(per_page.max(1)) {
            pages.push(chunk.to_vec());
        }
    }
    // A trailing form feed, or an empty file, leaves pages with nothing on them.
    while pages.last().is_some_and(|p: &Vec<String>| p.iter().all(String::is_empty)) {
        pages.pop();
    }
    pages
}

/// Add a text file as pages. Returns how many.
fn add_text(builder: &mut Builder, name: &str, text: &str, size: PageSize) -> Result<usize, CommandError> {
    let (page_w, page_h) = size.portrait().unwrap_or((595.28, 841.89));
    let width = page_w - 2.0 * TEXT_MARGIN;
    let per_page = ((page_h - 2.0 * TEXT_MARGIN) / LEADING).floor().max(1.0);
    // At most ~60 lines a page, so the conversion is exact.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let per_page = per_page as usize;

    let font_bytes = pick_text_font(name, text)?;
    let font: CidFont = build_cid_font(&mut builder.doc, &font_bytes, text)?;
    let measure = |s: &str| font.width(s, TEXT_SIZE);
    let pages = lay_out(text, width, per_page, &measure);
    if pages.is_empty() {
        return Ok(0);
    }
    let font_ref = builder.doc.add_object(font.font_dict.clone());

    for lines in &pages {
        let mut content = format!(
            "BT /F1 {} Tf {} TL {} {} Td\n",
            num(TEXT_SIZE),
            num(LEADING),
            num(TEXT_MARGIN),
            num(page_h - TEXT_MARGIN - TEXT_SIZE)
        );
        for (index, line) in lines.iter().enumerate() {
            if index > 0 {
                content.push_str("T*\n");
            }
            if !line.is_empty() {
                let _ = writeln!(content, "<{}> Tj", font.encode_hex(line));
            }
        }
        content.push_str("ET");
        builder.add_page((page_w, page_h), &content, dictionary! { "Font" => dictionary! { "F1" => font_ref } });
    }
    Ok(pages.len())
}

// ---------------------------------------------------------------------------

enum Source {
    Image(ImageFormat),
    Text,
}

fn classify(path: &Path, bytes: &[u8]) -> Result<Source, CommandError> {
    let extension = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if TEXT_EXTENSIONS.contains(&extension.as_str()) {
        return Ok(Source::Text);
    }
    match image::guess_format(bytes) {
        Ok(format @ (ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::Tiff | ImageFormat::WebP)) => {
            Ok(Source::Image(format))
        }
        _ if matches!(extension.as_str(), "docx" | "html" | "htm") => Err(CommandError::InvalidInput(format!(
            "{} is a Word or HTML file, which can't be turned into a PDF yet.",
            file_label(path)
        ))),
        _ => Err(CommandError::InvalidInput(format!(
            "{} isn't a JPEG, PNG, TIFF or WebP image or a plain-text file.",
            file_label(path)
        ))),
    }
}

/// SPEC: P7-OCR-009 — the new PDF's bytes for `sources`, in order. Reads the
/// files; writes nothing.
///
/// # Errors
/// When a file can't be read or isn't a supported kind, a text character has
/// no font, or nothing at all would be on the pages.
pub fn build_pdf(sources: &[PathBuf], size: PageSize) -> Result<(Vec<u8>, CreateSummary), CommandError> {
    if sources.is_empty() {
        return Err(CommandError::InvalidInput("Choose at least one file.".into()));
    }
    let mut builder = Builder::new();
    let mut summary = CreateSummary::default();
    for path in sources {
        let name = file_label(path);
        let bytes = std::fs::read(path)
            .map_err(|e| CommandError::InvalidInput(format!("{name} could not be read: {e}")))?;
        match classify(path, &bytes)? {
            Source::Image(format) => {
                add_image(&mut builder, &name, &bytes, format, size)?;
                summary.images += 1;
            }
            Source::Text => {
                let text = decode_text(&name, &bytes, &mut summary.warnings);
                if add_text(&mut builder, &name, &text, size)? == 0 {
                    summary.warnings.push(format!("{name} is empty, so it added no pages."));
                }
                summary.text_files += 1;
            }
        }
    }
    if builder.kids.is_empty() {
        return Err(CommandError::InvalidInput("Those files have nothing to put on a page.".into()));
    }
    summary.pages = u32::try_from(builder.kids.len()).unwrap_or(u32::MAX);
    Ok((builder.finish()?, summary))
}

/// SPEC: P7-OCR-009 — create the PDF at `dest` from `sources`, through the
/// verified save path (temporary file, reopened, renamed into place).
///
/// # Errors
/// As [`build_pdf`], or when the result does not load or save cleanly.
pub fn create_pdf_from_files(sources: &[PathBuf], dest: &Path, size: PageSize) -> Result<CreateSummary, CommandError> {
    let (bytes, mut summary) = build_pdf(sources, size)?;
    let doc = {
        let _guard = pdfium_lock()?;
        load_edited_bytes(bytes)?
    };
    let saved = save_document(&doc, dest, false, None);
    {
        // Closing a document is a PDFium call like any other.
        let _guard = pdfium_lock()?;
        drop(doc);
    }
    summary.bytes = saved?.bytes_written;
    Ok(summary)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    fn monospace(s: &str) -> f32 {
        s.chars().count() as f32
    }

    #[test]
    fn a_jpeg_header_is_read_without_decoding() {
        // SOI, APP14 "Adobe", SOF0 (8-bit, 2 x 3, 4 components), EOI.
        let mut bytes = vec![0xFF, 0xD8, 0xFF, 0xEE, 0x00, 0x0E];
        bytes.extend_from_slice(b"Adobe\0\x64\0\0\0\0\x02");
        bytes.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x08, 8, 0, 3, 0, 2, 4, 0xFF, 0xD9]);
        let info = jpeg_info(&bytes).expect("a header");
        assert_eq!(info, JpegInfo { width: 2, height: 3, components: 4, precision: 8, frame: 0xC0, adobe: true });
        assert!(passes_through(&info));
    }

    #[test]
    fn what_dct_decode_cannot_read_is_decoded_instead() {
        let base = JpegInfo { width: 2, height: 2, components: 3, precision: 8, frame: 0xC0, adobe: false };
        assert!(!passes_through(&JpegInfo { precision: 12, ..base }), "12-bit");
        assert!(!passes_through(&JpegInfo { frame: 0xC9, ..base }), "arithmetic coding");
        assert!(!passes_through(&JpegInfo { components: 2, ..base }));
        assert!(passes_through(&JpegInfo { frame: 0xC2, ..base }), "progressive is fine");
    }

    #[test]
    fn an_image_is_scaled_down_never_up_and_turns_the_page() {
        // A4 portrait for a tall image, landscape for a wide one.
        let ((pw, ph), [_, _, w, h]) = image_page(PageSize::A4, (4000.0, 3000.0));
        assert!(pw > ph, "a wide image gets a landscape page");
        assert!((w / h - 4.0 / 3.0).abs() < 1e-3, "aspect kept");
        assert!(w <= pw - 2.0 * IMAGE_MARGIN + 0.01);
        let ((pw, ph), [x, y, w, h]) = image_page(PageSize::A4, (50.0, 100.0));
        assert_eq!((w, h), (50.0, 100.0), "a small image keeps its size");
        assert!((x - (pw - 50.0) / 2.0).abs() < 1e-3 && (y - (ph - 100.0) / 2.0).abs() < 1e-3, "and is centred");
        let (page, [x, y, w, h]) = image_page(PageSize::Image, (640.0, 480.0));
        assert_eq!((page, [x, y, w, h]), ((640.0, 480.0), [0.0, 0.0, 640.0, 480.0]));
    }

    #[test]
    fn long_lines_wrap_at_spaces_and_long_words_break() {
        let lines = wrap("the quick brown fox jumps", 10.0, &monospace);
        assert_eq!(lines, vec!["the quick", "brown fox", "jumps"]);
        let lines = wrap("abcdefghijklmnop", 5.0, &monospace);
        assert_eq!(lines, vec!["abcde", "fghij", "klmno", "p"]);
    }

    #[test]
    fn cjk_breaks_between_characters() {
        let lines = wrap("日本語のテキストです", 4.0, &monospace);
        assert_eq!(lines, vec!["日本語の", "テキスト", "です"]);
    }

    #[test]
    fn tabs_expand_to_eight_column_stops() {
        assert_eq!(clean_line("a\tb"), "a       b");
        assert_eq!(clean_line("abcdefgh\tc"), "abcdefgh        c");
        assert_eq!(clean_line("x\u{7}y"), "xy", "a bell character is not text");
    }

    #[test]
    fn a_form_feed_starts_a_page_and_pages_fill() {
        let pages = lay_out("one\ntwo\nthree\u{c}four\n", 100.0, 2, &monospace);
        assert_eq!(pages, vec![vec!["one", "two"], vec!["three"], vec!["four"]]);
        assert!(lay_out("", 100.0, 2, &monospace).is_empty(), "an empty file has no pages");
        assert_eq!(lay_out("a\r\nb\rc", 100.0, 9, &monospace), vec![vec!["a", "b", "c"]], "CR LF and CR end lines too");
    }

    #[test]
    fn text_encodings_are_recognised() {
        let mut warnings = Vec::new();
        assert_eq!(decode_text("a", "naïve".as_bytes(), &mut warnings), "naïve");
        assert_eq!(decode_text("a", b"\xEF\xBB\xBFhi", &mut warnings), "hi");
        assert_eq!(decode_text("a", &[0xFF, 0xFE, b'h', 0, b'i', 0], &mut warnings), "hi");
        assert_eq!(decode_text("a", &[0xFE, 0xFF, 0, b'h', 0, b'i'], &mut warnings), "hi");
        assert!(warnings.is_empty());
        assert_eq!(decode_text("old.txt", b"caf\xE9 \x80 \x93", &mut warnings), "café € “");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].starts_with("old.txt is not UTF-8"));
    }

    #[test]
    fn numbers_are_written_compactly() {
        assert_eq!(num(12.0), "12");
        assert_eq!(num(0.5), "0.5");
        assert_eq!(num(-0.0001), "0");
        assert_eq!(num(841.89), "841.89");
    }
}
