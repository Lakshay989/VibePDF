//! SPEC: P7-OCR-010 — compressing a document.
//!
//! Two kinds of test here, and the split is deliberate.
//!
//! The **correctness** tests run against the small committed fixtures and check
//! the rules that make compression safe to offer at all: a bilevel scan is not
//! turned into a JPEG, an indexed palette is not reinterpreted as colour, a
//! file never grows, and the result still opens with the same pages. They run
//! on every clone.
//!
//! The **ratio** tests need a document that can actually be compressed, and a
//! fixture like that is tens of megabytes by nature — Flate cannot squeeze
//! sensor noise, which is the entire reason scans are large. That fixture is
//! gitignored, so those tests skip with a printed instruction when it is
//! absent rather than silently passing.

use std::path::PathBuf;

use vibepdf_lib::pdf::compress::{compress_bytes, CompressLevel};
use vibepdf_lib::pdf::document::open_pdf;

fn fixture(name: &str) -> PathBuf {
    let p = PathBuf::from("../tests/fixtures/basic").join(name);
    assert!(p.is_file(), "fixture missing at {}", p.display());
    p
}

fn bytes_of(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).expect("read fixture")
}

/// Does `haystack` contain `needle`? Used to assert a filter name survived.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// The scan is gitignored because it is ~28 MB. Skip loudly, never silently.
fn scan_or_skip() -> Option<Vec<u8>> {
    let path = PathBuf::from("../tests/fixtures/basic/noisy-scan.pdf");
    if path.is_file() {
        return Some(std::fs::read(path).expect("read scan"));
    }
    eprintln!(
        "skipping: fixture missing at {}\n  regenerate with: \
         python3 tests/fixtures/basic/generate-noisy-scan.py --pages 6",
        path.display()
    );
    None
}

/// Page count and page sizes, read back through `PDFium` — the check that the
/// output is a real PDF and not merely smaller bytes.
///
/// Goes through the crate's own loader rather than binding `PDFium` here: the
/// bindings are process-global and initialising a second set panics with
/// `PdfiumLibraryBindingsAlreadyInitialized`.
fn page_geometry(bytes: &[u8]) -> Vec<(f32, f32)> {
    let dir = std::env::temp_dir().join(format!("vibepdf-compress-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("out.pdf");
    std::fs::write(&path, bytes).expect("write");
    let (doc, _) = open_pdf(&path, None).expect("the output opens");
    let sizes = doc
        .pages()
        .iter()
        .map(|p| (p.width().value, p.height().value))
        .collect();
    drop(doc);
    let _ = std::fs::remove_dir_all(&dir);
    sizes
}

#[test]
fn a_scan_compresses_at_every_level_and_still_opens() {
    // SPEC: P7-OCR-010. The headline, and everything that has to hold with it,
    // in one pass per level — a pass over the scan is tens of seconds in a
    // debug build, so the assertions share them rather than each paying again.
    //
    // Asserted here: every level shrinks the file materially; the levels are
    // ordered (a "high" that does not beat "low" is a lie in the dialog); the
    // report matches the real sizes; and the output still opens with the same
    // pages at the same size.
    let Some(bytes) = scan_or_skip() else { return };
    let before = page_geometry(&bytes);

    let mut sizes = Vec::new();
    let mut medium_output = Vec::new();
    for level in [CompressLevel::Low, CompressLevel::Medium, CompressLevel::High] {
        let (out, report) = compress_bytes(&bytes, level).expect("compress");
        assert!(
            out.len() < bytes.len() / 2,
            "{level:?} barely helped: {} -> {}",
            bytes.len(),
            out.len()
        );
        assert_eq!(report.before_bytes, bytes.len() as u64);
        assert_eq!(report.after_bytes, out.len() as u64);
        assert!(report.images_recompressed > 0, "{level:?} recompressed nothing");
        assert_eq!(page_geometry(&out), before, "{level:?} changed the pages");
        sizes.push(out.len());
        if level == CompressLevel::Medium {
            medium_output = out;
        }
    }
    assert!(sizes[2] < sizes[1], "high {} should beat medium {}", sizes[2], sizes[1]);
    assert!(sizes[1] < sizes[0], "medium {} should beat low {}", sizes[1], sizes[0]);

    // The rule that makes this safe to run on anything: without the "only if
    // smaller" guards a second pass re-encodes already-lossy JPEGs, and the
    // file creeps upwards while the picture gets worse.
    let (twice, _) = compress_bytes(&medium_output, CompressLevel::Medium).expect("second pass");
    assert!(
        twice.len() <= medium_output.len(),
        "a second pass grew the file: {} -> {}",
        medium_output.len(),
        twice.len()
    );
}

#[test]
fn an_image_that_jpeg_cannot_beat_keeps_its_own_bytes() {
    // SPEC: P7-OCR-010. `scan.pdf` is a 1275x1650 grayscale page that is mostly
    // white, which Flate takes down to 5.2 KB — far smaller than any JPEG of
    // the same pixels. The per-image "only if smaller" guard has to fire here
    // and leave the original data in place.
    //
    // This fixture, not the noisy one: on a real scan JPEG always wins, so the
    // guard never runs and a test there cannot tell whether it exists. The
    // whole-document guard cannot stand in for it either — it only sees the
    // total, and would happily ship a file whose images each grew while the
    // deflation of something else paid for it.
    let bytes = bytes_of("scan.pdf");
    let (out, report) = compress_bytes(&bytes, CompressLevel::Low).expect("compress");
    assert_eq!(
        report.images_recompressed, 0,
        "re-encoded an image that Flate already stored more tightly"
    );
    assert_eq!(report.images_untouched, 1);
    // The original Flate image data is still there, not a JPEG.
    assert!(contains(&out, b"FlateDecode"));
    assert!(!contains(&out, b"DCTDecode"));
    // And — the part that distinguishes this from the whole-document fallback —
    // the file was still *tidied*: it came back smaller than the input rather
    // than being handed back verbatim. Skipping one hopeless image is not the
    // same as giving up on the document, and only the per-image guard can tell
    // the difference; without it the document grows, the whole-document guard
    // fires, and the original bytes are returned unchanged.
    assert!(
        out.len() < bytes.len(),
        "expected a small saving from re-serialization: {} -> {}",
        bytes.len(),
        out.len()
    );
    assert_ne!(out, bytes, "the whole document was abandoned, not one image");
}

#[test]
fn a_bilevel_scan_is_left_alone() {
    // SPEC: P7-OCR-010. ccitt.pdf is a 1-bit black-and-white image. JPEG is the
    // wrong codec for it — it would add grey fringes to pure black on white and
    // usually grow the file — so it must be reported untouched, not recompressed.
    let bytes = bytes_of("ccitt.pdf");
    let (out, report) = compress_bytes(&bytes, CompressLevel::High).expect("compress");
    assert_eq!(report.images_recompressed, 0, "recompressed a bilevel image");
    assert_eq!(report.images_untouched, 1);
    assert!(!out.is_empty());
    // And the image keeps its own filter rather than becoming a JPEG.
    assert!(contains(&out, b"CCITTFaxDecode"), "the CCITT filter was rewritten");
    assert!(!contains(&out, b"DCTDecode"), "a bilevel image was turned into a JPEG");
}

#[test]
fn a_jpeg2000_image_is_left_alone() {
    // We have no JPEG 2000 decoder and will not guess at one.
    let bytes = bytes_of("jpx.pdf");
    let (out, report) = compress_bytes(&bytes, CompressLevel::High).expect("compress");
    assert_eq!(report.images_recompressed, 0);
    assert!(contains(&out, b"JPXDecode"), "the JPX filter was rewritten");
    assert!(!contains(&out, b"DCTDecode"), "a JPEG 2000 image was re-encoded");
}

#[test]
fn stream_deflation_is_where_a_text_document_wins() {
    // SPEC: P7-OCR-010's third target. unicode-text.pdf stores its embedded
    // font program uncompressed; deflation alone is worth ~93% of the file.
    // Measured 2026-09-23.
    let bytes = bytes_of("unicode-text.pdf");
    let (out, report) = compress_bytes(&bytes, CompressLevel::Low).expect("compress");
    assert!(report.streams_deflated >= 5, "deflated {}", report.streams_deflated);
    assert_eq!(report.images_recompressed, 0, "there are no images in this file");
    assert!(
        out.len() < bytes.len() / 4,
        "expected a large win from deflation alone: {} -> {}",
        bytes.len(),
        out.len()
    );
    assert_eq!(page_geometry(&out).len(), 1);
}

#[test]
fn a_document_of_tiny_streams_is_not_made_worse() {
    // many-pages.pdf is 50 content streams of a dozen bytes each. Deflating one
    // costs more in the `/Filter /FlateDecode` it must add than it saves, so
    // none of them should be touched — measured 2026-09-23, doing it anyway
    // grows the file by 2.2%.
    let bytes = bytes_of("many-pages.pdf");
    let (out, report) = compress_bytes(&bytes, CompressLevel::High).expect("compress");
    assert_eq!(report.streams_deflated, 0, "deflated streams that should have been skipped");
    assert!(out.len() <= bytes.len(), "the file grew: {} -> {}", bytes.len(), out.len());
    assert_eq!(page_geometry(&out).len(), 50);
}

#[test]
fn an_unknown_level_is_refused() {
    assert!(CompressLevel::parse("low").is_ok());
    assert!(CompressLevel::parse("medium").is_ok());
    assert!(CompressLevel::parse("high").is_ok());
    assert!(CompressLevel::parse("maximum").is_err());
    assert!(CompressLevel::parse("Low").is_err());
}

#[test]
fn a_page_that_is_not_a_scan_still_opens() {
    // The everyday case: a text PDF has nothing to recompress, and must come
    // back openable and unchanged in shape rather than subtly damaged.
    let bytes = bytes_of("two-column.pdf");
    let before = page_geometry(&bytes);
    let (out, _) = compress_bytes(&bytes, CompressLevel::High).expect("compress");
    assert_eq!(page_geometry(&out), before);
}

/// Writes a before/after set into `Sample PDFs/verify-compress/` so a human can
/// open both and judge the quality, which is the one thing a test cannot do.
///
/// Ignored: it writes outside the test scratch directory and needs the scan.
/// `cargo test --test compress -- --ignored writes_the_verification_set`
#[test]
#[ignore = "writes verification files for a human to open"]
fn writes_the_verification_set() {
    let Some(bytes) = scan_or_skip() else { return };
    let dir = PathBuf::from("../Sample PDFs/out/p7-compress");
    std::fs::create_dir_all(&dir).expect("verification dir");

    std::fs::write(dir.join("00-original.pdf"), &bytes).expect("write original");
    for (name, level) in [
        ("01-low", CompressLevel::Low),
        ("02-medium", CompressLevel::Medium),
        ("03-high", CompressLevel::High),
    ] {
        let (out, report) = compress_bytes(&bytes, level).expect("compress");
        std::fs::write(dir.join(format!("{name}.pdf")), &out).expect("write");
        #[allow(clippy::cast_precision_loss)]
        let share = report.after_bytes as f64 / report.before_bytes as f64 * 100.0;
        println!(
            "VERIFY {name}: {} -> {} bytes ({share:.1}%), {} images recompressed",
            report.before_bytes, report.after_bytes, report.images_recompressed
        );
    }
}
