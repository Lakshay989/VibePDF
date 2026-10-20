//! SPEC: P7-OCR-009 (P7.C1a) — create a PDF from image and plain-text files.
//!
//! Every input is generated here, so nothing depends on files that aren't in
//! the repo — except the CMYK JPEG, which Ghostscript makes (our encoder can't
//! write CMYK), and which skips loudly without it.
//!
//! What the tests trust: PDFium *renders* the output and its pixels are
//! sampled; PDFium and Ghostscript each *extract* the text and it is compared
//! with the source; lopdf reads the structure (fonts embedded, the JPEG
//! stream byte-identical). None of those is the code that wrote the file.

use std::path::{Path, PathBuf};
use std::process::Command;

use image::metadata::Orientation;
use image::{DynamicImage, ImageEncoder, Rgb, RgbImage};
use lopdf::{Document, Object};
use vibepdf_lib::error::CommandError;
use vibepdf_lib::pdf::create::{create_pdf_from_files, CreateSummary, PageSize};
use vibepdf_lib::pdf::document::open_pdf;
use vibepdf_lib::pdf::render::{render_page, ImageFormat};
use vibepdf_lib::pdf::text_extract::extract_text_runs;

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("vibepdf-create-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Self(dir)
    }

    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, bytes).expect("write input");
        path
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// --- inputs -----------------------------------------------------------------

/// Four solid quadrants — red, green / blue, white — so any turn or flip moves
/// a colour somewhere it can be seen.
fn quadrants(width: u32, height: u32) -> RgbImage {
    RgbImage::from_fn(width, height, |x, y| {
        let (right, bottom) = (x >= width / 2, y >= height / 2);
        match (right, bottom) {
            (false, false) => Rgb([220, 30, 30]),
            (true, false) => Rgb([30, 180, 30]),
            (false, true) => Rgb([30, 30, 220]),
            (true, true) => Rgb([250, 250, 250]),
        }
    })
}

fn jpeg(image: &RgbImage) -> Vec<u8> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 95)
        .write_image(image.as_raw(), image.width(), image.height(), image::ExtendedColorType::Rgb8)
        .expect("encode jpeg");
    out
}

/// `jpeg` with an EXIF APP1 segment giving `orientation` (1–8), spliced in
/// after the start-of-image marker — what a phone writes.
fn jpeg_with_orientation(image: &RgbImage, orientation: u16) -> Vec<u8> {
    let plain = jpeg(image);
    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"II*\0");
    tiff.extend_from_slice(&8u32.to_le_bytes()); // IFD0 offset
    tiff.extend_from_slice(&1u16.to_le_bytes()); // one entry
    tiff.extend_from_slice(&0x0112u16.to_le_bytes()); // Orientation
    tiff.extend_from_slice(&3u16.to_le_bytes()); // SHORT
    tiff.extend_from_slice(&1u32.to_le_bytes()); // count
    tiff.extend_from_slice(&orientation.to_le_bytes());
    tiff.extend_from_slice(&[0, 0]);
    tiff.extend_from_slice(&0u32.to_le_bytes()); // no next IFD
    let mut payload = b"Exif\0\0".to_vec();
    payload.extend_from_slice(&tiff);
    let length = u16::try_from(payload.len() + 2).expect("small");
    let mut out = vec![0xFF, 0xD8, 0xFF, 0xE1];
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(&payload);
    out.extend_from_slice(&plain[2..]);
    out
}

fn png(width: u32, height: u32, color: png::ColorType, pixels: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(color);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().expect("header").write_image_data(pixels).expect("pixels");
    }
    out
}

// --- reading the output -----------------------------------------------------

fn create(sources: &[PathBuf], dest: &Path, size: PageSize) -> CreateSummary {
    create_pdf_from_files(sources, dest, size).expect("create succeeds")
}

/// Page `index` rendered at 72 dpi (one pixel a point): width, height, RGBA.
fn render(path: &Path, index: u32) -> (u32, u32, Vec<u8>) {
    let (doc, _) = open_pdf(path, None).expect("opens");
    let page = render_page(&doc, index, 72.0, ImageFormat::Rgba8).expect("renders");
    (page.width, page.height, page.bytes)
}

fn pixel(rendered: &(u32, u32, Vec<u8>), x: u32, y: u32) -> [u8; 3] {
    let at = usize::try_from((y * rendered.0 + x) * 4).expect("index");
    [rendered.2[at], rendered.2[at + 1], rendered.2[at + 2]]
}

fn close_to(actual: [u8; 3], expected: [u8; 3]) -> bool {
    actual.iter().zip(expected).all(|(a, e)| a.abs_diff(e) <= 40)
}

fn page_count(path: &Path) -> usize {
    Document::load_mem(&std::fs::read(path).expect("read")).expect("parse").get_pages().len()
}

/// PDFium's text for every page, whitespace collapsed.
fn extracted_text(path: &Path) -> String {
    let (doc, meta) = open_pdf(path, None).expect("opens");
    let mut text = String::new();
    for page in 0..usize::try_from(meta.page_count).expect("count") {
        for run in extract_text_runs(&doc, page).expect("extract") {
            text.push_str(&run.text);
            text.push(' ');
        }
    }
    collapse(&text)
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn ghostscript() -> Option<&'static str> {
    ["/opt/homebrew/bin/gs", "/usr/local/bin/gs", "gs"]
        .into_iter()
        .find(|p| Command::new(p).arg("--version").output().is_ok_and(|o| o.status.success()))
}

// --- images -----------------------------------------------------------------

#[test]
fn each_image_is_a_page_in_the_order_given() {
    let scratch = Scratch::new("order");
    let wide = scratch.write("wide.png", &png(300, 100, png::ColorType::Rgb, &quadrants(300, 100).into_raw()));
    let tall = scratch.write("tall.jpg", &jpeg(&quadrants(100, 300)));
    let gray = scratch.write("gray.png", &png(50, 50, png::ColorType::Grayscale, &[128; 2500]));
    let dest = scratch.path("out.pdf");

    let summary = create(&[wide, tall, gray], &dest, PageSize::A4);
    assert_eq!((summary.pages, summary.images, summary.text_files), (3, 3, 0));
    assert!(summary.warnings.is_empty(), "{:?}", summary.warnings);

    let (w0, h0, _) = render(&dest, 0);
    let (w1, h1, _) = render(&dest, 1);
    assert!(w0 > h0, "the wide image got a portrait page");
    assert!(h1 > w1, "the tall image got a landscape page");
    assert_eq!((w0, h0), (842, 595), "not an A4 page");
}

#[test]
fn a_jpeg_goes_in_byte_for_byte() {
    let scratch = Scratch::new("jpeg");
    let source = jpeg(&quadrants(64, 48));
    let input = scratch.write("photo.jpg", &source);
    let dest = scratch.path("out.pdf");
    create(&[input], &dest, PageSize::A4);

    let doc = Document::load_mem(&std::fs::read(&dest).expect("read")).expect("parse");
    let jpegs: Vec<&Vec<u8>> = doc
        .objects
        .values()
        .filter_map(|o| o.as_stream().ok())
        .filter(|s| s.dict.get(b"Filter").and_then(Object::as_name).is_ok_and(|f| f == b"DCTDecode"))
        .map(|s| &s.content)
        .collect();
    assert_eq!(jpegs.len(), 1, "one JPEG stream");
    assert_eq!(jpegs[0], &source, "the JPEG was re-encoded");
}

#[test]
fn colours_and_placement_come_out_right() {
    let scratch = Scratch::new("colours");
    let input = scratch.write("q.png", &png(200, 100, png::ColorType::Rgb, &quadrants(200, 100).into_raw()));
    let dest = scratch.path("out.pdf");
    create(&[input], &dest, PageSize::Image);

    let page = render(&dest, 0);
    assert_eq!((page.0, page.1), (200, 100), "the page is the image's size");
    assert!(close_to(pixel(&page, 50, 25), [220, 30, 30]), "top-left: {:?}", pixel(&page, 50, 25));
    assert!(close_to(pixel(&page, 150, 25), [30, 180, 30]), "top-right");
    assert!(close_to(pixel(&page, 50, 75), [30, 30, 220]), "bottom-left");
    assert!(close_to(pixel(&page, 150, 75), [250, 250, 250]), "bottom-right");
}

#[test]
fn transparency_survives_as_a_soft_mask() {
    let scratch = Scratch::new("alpha");
    // Left half opaque black, right half fully transparent black.
    let pixels: Vec<u8> =
        (0..40 * 40).flat_map(|i| if i % 40 < 20 { [0, 0, 0, 255] } else { [0, 0, 0, 0] }).collect();
    let input = scratch.write("alpha.png", &png(40, 40, png::ColorType::Rgba, &pixels));
    let dest = scratch.path("out.pdf");
    create(&[input], &dest, PageSize::Image);

    let doc = Document::load_mem(&std::fs::read(&dest).expect("read")).expect("parse");
    assert!(
        doc.objects.values().filter_map(|o| o.as_stream().ok()).any(|s| s.dict.has(b"SMask")),
        "no soft mask"
    );
    let page = render(&dest, 0);
    assert!(close_to(pixel(&page, 10, 20), [0, 0, 0]), "the opaque half");
    assert!(close_to(pixel(&page, 30, 20), [255, 255, 255]), "the transparent half shows the page");
}

#[test]
fn every_exif_orientation_is_shown_upright() {
    // A wide stored image, so a quarter turn also has to swap the page's sides.
    let stored = quadrants(80, 40);
    let cases = [
        (1, Orientation::NoTransforms),
        (2, Orientation::FlipHorizontal),
        (3, Orientation::Rotate180),
        (4, Orientation::FlipVertical),
        (5, Orientation::Rotate90FlipH),
        (6, Orientation::Rotate90),
        (7, Orientation::Rotate270FlipH),
        (8, Orientation::Rotate270),
    ];
    for (tag, orientation) in cases {
        // What it should look like, according to the image crate.
        let mut expected = DynamicImage::ImageRgb8(stored.clone());
        expected.apply_orientation(orientation);
        let expected = expected.to_rgb8();

        let scratch = Scratch::new("exif");
        let input = scratch.write("photo.jpg", &jpeg_with_orientation(&stored, tag));
        let dest = scratch.path("out.pdf");
        create(&[input], &dest, PageSize::Image);
        let page = render(&dest, 0);

        assert_eq!((page.0, page.1), (expected.width(), expected.height()), "EXIF {tag}: page size");
        for (fx, fy) in [(1, 1), (3, 1), (1, 3), (3, 3)] {
            let (x, y) = (expected.width() * fx / 4, expected.height() * fy / 4);
            let want = expected.get_pixel(x, y).0;
            assert!(close_to(pixel(&page, x, y), want), "EXIF {tag} at ({x}, {y}): {:?} != {want:?}", pixel(&page, x, y));
        }
    }
}

#[test]
fn a_cmyk_jpeg_keeps_its_colours() {
    let Some(gs) = ghostscript() else {
        eprintln!("skipping: Ghostscript is not installed");
        return;
    };
    let scratch = Scratch::new("cmyk");
    let input = scratch.path("red.jpg");
    // A 40 x 40 pt page filled with red, written as a CMYK JPEG.
    let status = Command::new(gs)
        .args(["-q", "-dNOPAUSE", "-dBATCH", "-sDEVICE=jpegcmyk", "-r72", "-g40x40"])
        .arg(format!("-sOutputFile={}", input.display()))
        .args(["-c", "1 0 0 setrgbcolor 0 0 40 40 rectfill showpage"])
        .status()
        .expect("runs gs");
    assert!(status.success());
    let header = std::fs::read(&input).expect("read");
    assert!(header.windows(5).any(|w| w == b"Adobe"), "Ghostscript's CMYK JPEG should carry the Adobe marker");

    let dest = scratch.path("out.pdf");
    create(&[input], &dest, PageSize::Image);
    let page = render(&dest, 0);
    let middle = pixel(&page, 20, 20);
    assert!(middle[0] > 180 && middle[1] < 90 && middle[2] < 90, "red came out as {middle:?}");
}

// --- text -------------------------------------------------------------------

const SAMPLE: &str = "Invoice 4821 — naïve café, Æsir and Øresund.\n\
Счёт номер 4821, документ от 14 марта.\n\
Ελληνικά: αβγ δεζ.\n\
\tIndented with a tab.\n";

#[test]
fn text_comes_back_out_as_it_went_in() {
    let scratch = Scratch::new("text");
    let input = scratch.write("notes.txt", SAMPLE.as_bytes());
    let dest = scratch.path("out.pdf");
    let summary = create(&[input], &dest, PageSize::A4);
    assert_eq!((summary.pages, summary.text_files, summary.images), (1, 1, 0));
    assert_eq!(extracted_text(&dest), collapse(SAMPLE), "PDFium reads something else");
}

#[test]
fn ghostscript_reads_the_same_text() {
    let Some(gs) = ghostscript() else {
        eprintln!("skipping: Ghostscript is not installed");
        return;
    };
    let scratch = Scratch::new("gs-text");
    let input = scratch.write("notes.txt", SAMPLE.as_bytes());
    let dest = scratch.path("out.pdf");
    create(&[input], &dest, PageSize::A4);
    let output = Command::new(gs)
        .args(["-q", "-dNOPAUSE", "-dBATCH", "-sDEVICE=txtwrite", "-sOutputFile=-"])
        .arg(&dest)
        .output()
        .expect("runs gs");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(collapse(&String::from_utf8_lossy(&output.stdout)), collapse(SAMPLE));
}

#[test]
fn long_text_wraps_inside_the_margin_and_fills_pages() {
    let scratch = Scratch::new("long");
    let long_line = "word ".repeat(60); // ~300 characters
    let text: String = (1..=150).map(|n| format!("line {n}\n")).collect::<String>() + &long_line + "\n";
    let input = scratch.write("long.txt", text.as_bytes());
    let dest = scratch.path("out.pdf");
    let summary = create(&[input], &dest, PageSize::A4);
    // A4 at 10/12 pt with 54 pt margins holds 61 lines: 150 lines, then a
    // 300-character line that wraps onto four more.
    assert_eq!(summary.pages, 3);
    assert_eq!(extracted_text(&dest), collapse(&text), "text was lost in the wrapping");

    let (doc, _) = open_pdf(&dest, None).expect("opens");
    for page in 0..3 {
        for run in extract_text_runs(&doc, page).expect("extract") {
            assert!(run.bbox[2] <= 595.28 - 54.0 + 1.0, "text runs past the right margin: {:?}", run.bbox);
        }
    }
}

#[test]
fn a_form_feed_starts_a_new_page() {
    let scratch = Scratch::new("ff");
    let input = scratch.write("pages.txt", b"first page\n\x0csecond page\n");
    let dest = scratch.path("out.pdf");
    assert_eq!(create(&[input], &dest, PageSize::Letter).pages, 2);
    let (w, h, _) = render(&dest, 0);
    assert_eq!((w, h), (612, 792), "not a Letter page");
}

#[test]
fn fonts_are_embedded() {
    let scratch = Scratch::new("fonts");
    let input = scratch.write("notes.txt", SAMPLE.as_bytes());
    let dest = scratch.path("out.pdf");
    create(&[input], &dest, PageSize::A4);

    let doc = Document::load_mem(&std::fs::read(&dest).expect("read")).expect("parse");
    let descriptors: Vec<_> = doc
        .objects
        .values()
        .filter_map(|o| o.as_dict().ok())
        .filter(|d| d.get(b"Type").and_then(Object::as_name).is_ok_and(|t| t == b"FontDescriptor"))
        .collect();
    assert!(!descriptors.is_empty(), "no fonts at all");
    for descriptor in descriptors {
        assert!(descriptor.has(b"FontFile2"), "a font is not embedded: {descriptor:?}");
    }
}

#[test]
fn text_that_is_not_utf8_is_read_as_windows_1252_with_a_warning() {
    let scratch = Scratch::new("1252");
    let input = scratch.write("old.txt", b"Caf\xe9 \x93quoted\x94 \x80 5\n");
    let dest = scratch.path("out.pdf");
    let summary = create(&[input], &dest, PageSize::A4);
    assert_eq!(summary.warnings.len(), 1, "{:?}", summary.warnings);
    assert_eq!(extracted_text(&dest), "Café “quoted” € 5");
}

#[test]
fn images_and_text_combine_in_order() {
    let scratch = Scratch::new("mixed");
    let a = scratch.write("a.txt", b"before the picture\n");
    let b = scratch.write("b.png", &png(20, 20, png::ColorType::Grayscale, &[0; 400]));
    let c = scratch.write("c.txt", b"after the picture\n");
    let dest = scratch.path("out.pdf");
    let summary = create(&[a, b, c], &dest, PageSize::A4);
    assert_eq!((summary.pages, summary.images, summary.text_files), (3, 1, 2));
    let (doc, _) = open_pdf(&dest, None).expect("opens");
    let first: String = extract_text_runs(&doc, 0).expect("extract").into_iter().map(|r| r.text).collect();
    let last: String = extract_text_runs(&doc, 2).expect("extract").into_iter().map(|r| r.text).collect();
    assert!(first.contains("before") && last.contains("after"), "{first:?} / {last:?}");
    assert!(extract_text_runs(&doc, 1).expect("extract").is_empty(), "the image page has text");
}

// --- large images --------------------------------------------------------------

/// A PNG whose header claims `width`×`height` RGBA, with an empty image
/// stream: enough to be judged by its size without decoding megabytes.
fn png_header_claiming(width: u32, height: u32) -> Vec<u8> {
    let chunk = |kind: &[u8], data: &[u8]| {
        let mut out = u32::try_from(data.len()).expect("len").to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let mut crc = crc32fast::Hasher::new();
        crc.update(kind);
        crc.update(data);
        out.extend_from_slice(&crc.finalize().to_be_bytes());
        out
    };
    let mut ihdr = width.to_be_bytes().to_vec();
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    out.extend(chunk(b"IHDR", &ihdr));
    out.extend(chunk(b"IDAT", &[0x78, 0x9C, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01]));
    out.extend(chunk(b"IEND", &[]));
    out
}

#[test]
fn a_twenty_megapixel_png_with_transparency_is_accepted() {
    // 5,000 x 4,000 RGBA is 80 MB decoded: over the png crate's default
    // 64 MiB decoding limit, well under the documented 100-megapixel cap.
    // A screenshot from a 5K display is this size.
    let scratch = Scratch::new("big-png");
    let (w, h) = (5_000u32, 4_000u32);
    let pixels: Vec<u8> = (0..w * h).flat_map(|i| if i % 2 == 0 { [40, 120, 200, 255] } else { [40, 120, 200, 128] }).collect();
    let input = scratch.write("screenshot.png", &png(w, h, png::ColorType::Rgba, &pixels));
    drop(pixels);
    let dest = scratch.path("out.pdf");
    let summary = create(&[input], &dest, PageSize::Image);
    assert_eq!((summary.pages, summary.images), (1, 1));
    let doc = Document::load_mem(&std::fs::read(&dest).expect("read")).expect("parse");
    let image = doc
        .objects
        .values()
        .filter_map(|o| o.as_stream().ok())
        .find(|s| s.dict.get(b"Subtype").and_then(Object::as_name).is_ok_and(|n| n == b"Image") && s.dict.has(b"SMask"))
        .expect("the image, with its soft mask");
    assert_eq!(image.dict.get(b"Width").and_then(Object::as_i64).expect("width"), 5_000);
}

#[test]
fn a_png_over_the_limit_is_refused_with_its_size() {
    let scratch = Scratch::new("huge-png");
    // 12,000 x 9,000 is 108 megapixels.
    let input = scratch.write("poster.png", &png_header_claiming(12_000, 9_000));
    let dest = scratch.path("out.pdf");
    let message = refusal(create_pdf_from_files(&[input], &dest, PageSize::A4));
    assert!(message.contains("12000 × 9000") && message.contains("100-megapixel limit"), "{message}");
    assert!(!dest.exists());
}

// --- refusals -----------------------------------------------------------------

fn refusal(result: Result<CreateSummary, CommandError>) -> String {
    match result {
        Err(CommandError::InvalidInput(message)) => message,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn an_unsupported_file_is_refused_by_name_and_nothing_is_written() {
    let scratch = Scratch::new("refuse");
    let good = scratch.write("ok.txt", b"fine\n");
    let bad = scratch.write("archive.zip", b"PK\x03\x04 not a document");
    let dest = scratch.path("out.pdf");
    let message = refusal(create_pdf_from_files(&[good, bad], &dest, PageSize::A4));
    assert!(message.starts_with("archive.zip isn't"), "{message}");
    assert!(!dest.exists(), "a refused creation wrote a file");
}

#[test]
fn a_character_no_font_can_draw_is_refused() {
    let scratch = Scratch::new("nofont");
    let input = scratch.write("pua.txt", "plane sixteen: \u{10FFFD}\n".as_bytes());
    let dest = scratch.path("out.pdf");
    let message = refusal(create_pdf_from_files(&[input], &dest, PageSize::A4));
    assert!(message.contains("U+10FFFD"), "{message}");
    assert!(!dest.exists());
}

#[test]
fn an_empty_text_file_adds_nothing_and_says_so() {
    let scratch = Scratch::new("empty");
    let empty = scratch.write("empty.txt", b"");
    let picture = scratch.write("p.png", &png(10, 10, png::ColorType::Grayscale, &[0; 100]));
    let dest = scratch.path("out.pdf");
    let summary = create(&[empty.clone(), picture], &dest, PageSize::A4);
    assert_eq!(summary.pages, 1);
    assert!(summary.warnings.iter().any(|w| w.starts_with("empty.txt is empty")), "{:?}", summary.warnings);
    assert_eq!(page_count(&dest), 1);

    let only_empty = refusal(create_pdf_from_files(&[empty], &scratch.path("none.pdf"), PageSize::A4));
    assert!(only_empty.contains("nothing to put on a page"), "{only_empty}");
}

/// Verification files for Preview: `Sample PDFs/out/p7-create/`.
/// `cargo test --test create_pdf -- --ignored writes_the_verification_files`
#[test]
#[ignore = "writes verification files for a human to open"]
fn writes_the_verification_files() {
    let dir = PathBuf::from("../Sample PDFs/out/p7-create");
    std::fs::create_dir_all(&dir).expect("dir");
    let inputs = Scratch::new("verify");
    let photo = inputs.write("photo-exif6.jpg", &jpeg_with_orientation(&quadrants(800, 400), 6));
    let shot = inputs.write("screenshot.png", &png(600, 300, png::ColorType::Rgb, &quadrants(600, 300).into_raw()));
    let notes = inputs.write("notes.txt", SAMPLE.as_bytes());
    let summary = create(&[photo, shot, notes], &dir.join("created.pdf"), PageSize::A4);
    println!("VERIFY {}: {summary:?}", dir.join("created.pdf").display());
}
