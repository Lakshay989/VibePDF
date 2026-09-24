//! SPEC: P7-OCR-005 — exporting pages as PNG, JPG, TIFF or WebP at 72–600 DPI.
//!
//! The pure parts (naming, validation, alpha flattening, encoder choice) are
//! unit-tested in `pdf::export_image`. These tests run whole documents through
//! PDFium and write real files, then **decode those files again** — an encoder
//! that writes a valid-looking header over a blank page passes a
//! "does the file exist" check and fails here.

use std::path::{Path, PathBuf};

use image::ImageFormat as DecodeFormat;
use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::pdf::export_image::ImageExportSummary;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn open(name: &str) -> DocumentActorHandle {
    DocumentActorHandle::spawn(None, uuid::Uuid::new_v4(), fixture(name), None).expect("opens")
}

/// A fresh empty directory that the caller deletes when it is done.
fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vibepdf-image-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

async fn export(
    name: &str,
    dir: &Path,
    pages: Vec<i32>,
    format: &str,
    dpi: f32,
) -> ImageExportSummary {
    open(name)
        .export_images(
            pages,
            dir.to_path_buf(),
            "doc".into(),
            vibepdf_lib::pdf::export_image::ImageExportOptions {
                format: vibepdf_lib::pdf::export_image::ImageExportFormat::parse(format)
                    .expect("known format"),
                dpi,
                quality: 85,
            },
        )
        .await
        .expect("export succeeds")
}

/// PNG is decoded with the `png` crate rather than `image` — we build `image`
/// without its png feature, because render.rs already encodes PNG directly.
fn png_size(path: &Path) -> (u32, u32) {
    let decoder = png::Decoder::new(std::fs::File::open(path).expect("open png"));
    let reader = decoder.read_info().expect("png header");
    let info = reader.info();
    (info.width, info.height)
}

fn png_pixels(path: &Path) -> (Vec<u8>, usize) {
    let decoder = png::Decoder::new(std::fs::File::open(path).expect("open png"));
    let mut reader = decoder.read_info().expect("png header");
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).expect("png data");
    let channels = info.color_type.samples();
    buf.truncate(info.buffer_size());
    (buf, channels)
}

#[tokio::test]
async fn exports_a_png_at_the_requested_dpi() {
    // SPEC: P7-OCR-005. hello.pdf is US Letter, 612 x 792 pt. At 150 DPI that
    // is 612/72*150 = 1275 px wide. If DPI were ignored we would get 612.
    let dir = scratch("dpi");
    let summary = export("hello.pdf", &dir, vec![], "png", 150.0).await;

    assert_eq!(summary.pages, 1);
    assert_eq!(summary.files, vec!["doc-001.png"]);
    assert!(summary.bytes > 0, "wrote an empty file");

    let (w, h) = png_size(&dir.join("doc-001.png"));
    assert_eq!((w, h), (1275, 1650), "DPI did not reach the renderer");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn dpi_scales_the_output() {
    // Two DPIs, one page: the pixel count must follow the DPI, not the page.
    let dir = scratch("scale");
    export("hello.pdf", &dir, vec![0], "png", 72.0).await;
    let low = png_size(&dir.join("doc-001.png"));
    std::fs::remove_file(dir.join("doc-001.png")).expect("remove");
    export("hello.pdf", &dir, vec![0], "png", 300.0).await;
    let high = png_size(&dir.join("doc-001.png"));

    assert_eq!(low, (612, 792), "72 DPI should be one pixel per point");
    assert_eq!(high, (2550, 3300));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn exports_all_four_formats_at_the_same_size() {
    // SPEC: P7-OCR-005 names PNG, JPG, TIFF and WebP. Each one is decoded
    // again here: a file that is not really an image of the page fails.
    let dir = scratch("formats");
    let png = export("hello.pdf", &dir, vec![0], "png", 72.0).await;
    assert_eq!(png.files, vec!["doc-001.png"]);
    assert_eq!(png_size(&dir.join("doc-001.png")), (612, 792));

    for (format, file, decode) in [
        ("jpeg", "doc-001.jpg", DecodeFormat::Jpeg),
        ("tiff", "doc-001.tiff", DecodeFormat::Tiff),
        ("webp", "doc-001.webp", DecodeFormat::WebP),
    ] {
        let summary = export("hello.pdf", &dir, vec![0], format, 72.0).await;
        assert_eq!(summary.files, vec![file], "wrong file name for {format}");
        let bytes = std::fs::read(dir.join(file)).expect("read output");
        let decoded =
            image::load_from_memory_with_format(&bytes, decode).expect("output decodes again");
        assert_eq!(
            (decoded.width(), decoded.height()),
            (612, 792),
            "{format} decoded to the wrong size"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn jpeg_output_is_opaque_rgb() {
    // JPEG is three-channel, so the RGBA render has to be composited down; a
    // four-channel buffer handed to the encoder is a refusal, and dropping the
    // channel instead of compositing would be a silent colour shift. The
    // margin of hello.pdf is white and must stay white.
    let dir = scratch("jpeg");
    export("hello.pdf", &dir, vec![0], "jpeg", 72.0).await;
    let bytes = std::fs::read(dir.join("doc-001.jpg")).expect("read output");
    let decoded = image::load_from_memory_with_format(&bytes, DecodeFormat::Jpeg).expect("decodes");
    assert_eq!(
        decoded.color(),
        image::ColorType::Rgb8,
        "JPEG should have no alpha channel"
    );
    let rgb = decoded.to_rgb8();
    let corner = rgb.get_pixel(4, 4).0;
    assert!(
        corner.iter().all(|&c| c > 240),
        "the page margin should be white, got {corner:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn the_export_contains_the_pages_ink() {
    // The failure this catches: an encoder that writes a correctly-sized,
    // perfectly blank image. ccitt.pdf is a black-and-white pattern, so the
    // output must hold both dark and light pixels.
    let dir = scratch("ink");
    export("ccitt.pdf", &dir, vec![0], "png", 300.0).await;
    let (pixels, channels) = png_pixels(&dir.join("doc-001.png"));
    let dark = pixels.chunks_exact(channels).filter(|p| p[0] < 64).count();
    let light = pixels.chunks_exact(channels).filter(|p| p[0] > 192).count();
    assert!(dark > 100, "no ink in the export ({dark} dark pixels)");
    assert!(light > 100, "no paper in the export ({light} light pixels)");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn tiff_output_is_compressed() {
    // `image`'s own TiffEncoder writes every page uncompressed: one US-Letter
    // page at 150 DPI came out 8.4 MB (measured 2026-09-23), which is 420 MB
    // for a 50-page export. `export_image` uses the `tiff` crate with Deflate
    // instead. The guard is generous — raw RGBA is 8.4 MB, so anything under a
    // quarter of that is certainly compressed — because the exact ratio is the
    // compressor's business, not ours.
    let dir = scratch("tiff-size");
    let summary = export("hello.pdf", &dir, vec![0], "tiff", 150.0).await;
    let raw = 1275_u64 * 1650 * 4;
    assert!(
        summary.bytes < raw / 4,
        "TIFF looks uncompressed: {} bytes against {raw} raw",
        summary.bytes
    );
    // Still a TIFF, and still the whole page.
    let bytes = std::fs::read(dir.join("doc-001.tiff")).expect("read output");
    let decoded = image::load_from_memory_with_format(&bytes, DecodeFormat::Tiff).expect("decodes");
    assert_eq!((decoded.width(), decoded.height()), (1275, 1650));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn files_are_named_by_the_pages_own_number() {
    // Exporting pages 3 and 5 (0-based 2 and 4) names them 003 and 005 — an
    // image file *is* that page, unlike a split file, which starts over at 1.
    let dir = scratch("naming");
    let summary = export("many-pages.pdf", &dir, vec![2, 4], "png", 72.0).await;
    assert_eq!(summary.pages, 2);
    assert_eq!(summary.files, vec!["doc-003.png", "doc-005.png"]);
    assert!(dir.join("doc-003.png").is_file());
    assert!(dir.join("doc-005.png").is_file());
    assert!(
        !dir.join("doc-001.png").exists(),
        "numbered by sequence instead of by page"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn an_empty_selection_exports_the_whole_document() {
    let dir = scratch("all");
    let summary = export("many-pages.pdf", &dir, vec![], "png", 72.0).await;
    assert_eq!(summary.pages, 50);
    assert_eq!(summary.files.len(), 50);
    assert_eq!(summary.files.first().map(String::as_str), Some("doc-001.png"));
    assert_eq!(summary.files.last().map(String::as_str), Some("doc-050.png"));
    let written = std::fs::read_dir(&dir).expect("read dir").count();
    assert_eq!(written, 50, "summary and directory disagree");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn dpi_outside_the_spec_range_is_refused() {
    // SPEC: P7-OCR-005 says 72–600. Outside it we refuse rather than clamp.
    let dir = scratch("range");
    for dpi in [71.0_f32, 601.0] {
        let err = open("hello.pdf")
            .export_images(
                vec![0],
                dir.clone(),
                "doc".into(),
                vibepdf_lib::pdf::export_image::ImageExportOptions {
                    dpi,
                    ..vibepdf_lib::pdf::export_image::ImageExportOptions::default()
                },
            )
            .await
            .expect_err("out-of-range DPI must be refused");
        let message = format!("{err:?}");
        assert!(
            message.contains("dpi"),
            "unhelpful error for {dpi} DPI: {message}"
        );
    }
    assert_eq!(
        std::fs::read_dir(&dir).expect("read dir").count(),
        0,
        "a refused export still wrote files"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_page_past_the_end_is_refused() {
    let dir = scratch("range-page");
    let err = open("hello.pdf")
        .export_images(
            vec![7],
            dir.clone(),
            "doc".into(),
            vibepdf_lib::pdf::export_image::ImageExportOptions::default(),
        )
        .await
        .expect_err("page 8 of a one-page document must be refused");
    assert!(format!("{err:?}").to_lowercase().contains("page"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn exports_at_the_top_of_the_dpi_range() {
    // 600 DPI on US Letter is 5100 x 6600 — 135 MB of RGBA for one page. This
    // is the memory ceiling the loop is written for; JPEG keeps the test quick.
    let dir = scratch("600");
    let summary = export("hello.pdf", &dir, vec![0], "jpeg", 600.0).await;
    assert_eq!(summary.pages, 1);
    let bytes = std::fs::read(dir.join("doc-001.jpg")).expect("read output");
    let decoded = image::load_from_memory_with_format(&bytes, DecodeFormat::Jpeg).expect("decodes");
    assert_eq!((decoded.width(), decoded.height()), (5100, 6600));
    let _ = std::fs::remove_dir_all(&dir);
}


/// Writes a set of real exports into `Sample PDFs/verify-images/` so a human
/// can open them in Preview, Finder and anything else, which is the only check
/// that proves the files are images to something other than our own decoder.
///
/// Ignored: it writes outside the test scratch directory.
/// `cargo test --test export_image -- --ignored writes_the_verification_set`
#[tokio::test]
#[ignore = "writes verification files for a human to open"]
async fn writes_the_verification_set() {
    let dir = PathBuf::from("../Sample PDFs/out/p7-images");
    std::fs::create_dir_all(&dir).expect("verification dir");

    // One page, each format, at a DPI worth eyeballing.
    for (format, dpi) in [("png", 150.0), ("jpeg", 150.0), ("tiff", 150.0), ("webp", 150.0)] {
        open("hello.pdf")
            .export_images(
                vec![0],
                dir.clone(),
                format!("hello-{format}"),
                vibepdf_lib::pdf::export_image::ImageExportOptions {
                    format: vibepdf_lib::pdf::export_image::ImageExportFormat::parse(format)
                        .expect("known format"),
                    dpi,
                    quality: 85,
                },
            )
            .await
            .expect("export succeeds");
    }

    // The two ends of the spec's range, so the difference is visible.
    export("ccitt.pdf", &dir, vec![0], "png", 72.0).await;
    std::fs::rename(dir.join("doc-001.png"), dir.join("ccitt-72dpi.png")).expect("rename");
    export("ccitt.pdf", &dir, vec![0], "png", 600.0).await;
    std::fs::rename(dir.join("doc-001.png"), dir.join("ccitt-600dpi.png")).expect("rename");

    // A colour page, to catch a channel-order mistake that grey text hides.
    export("jpx.pdf", &dir, vec![0], "png", 300.0).await;
    std::fs::rename(dir.join("doc-001.png"), dir.join("jpx-colour.png")).expect("rename");

    // Numbering across a real selection.
    export("many-pages.pdf", &dir, vec![2, 4], "jpeg", 150.0).await;

    println!("VERIFY: wrote to {}", dir.display());
}
