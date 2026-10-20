//! SPEC: P7-OCR-009 (P7.C1d) — "produce a PDF/A-compatible output when the
//! user requests it": PDF/A-2u from every kind of source.
//!
//! Two kinds of check. Structure, read with lopdf from the file as finally
//! written (after the PDFium save path): the output intent and its profile,
//! the XMP and the Info entries that must agree with it, the identifier, no
//! encryption, no `DeviceCMYK`, every font embedded with a Unicode map. And
//! conformance, judged by veraPDF, the PDF Association's reference validator,
//! which we did not write. veraPDF is a Java program installed apart from the
//! project (`brew install verapdf`); without it those checks skip, loudly —
//! unless `VIBEPDF_REQUIRE_VERAPDF` is set, as CI sets it, where a missing
//! validator fails instead of passing quietly.

use std::path::{Path, PathBuf};
use std::process::Command;

use lopdf::{Dictionary, Document, Object};
use vibepdf_lib::error::CommandError;
use vibepdf_lib::pdf::create::{build_pdf_with, create_pdf, CreateOptions, CreateSummary, PageSize};
use vibepdf_lib::pdf::document::open_pdf;
use vibepdf_lib::pdf::render::{render_page, ImageFormat};

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("vibepdf-pdfa-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Self(dir)
    }
    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, bytes).expect("write");
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

fn create(sources: &[PathBuf], dest: &Path, pdf_a: bool) -> CreateSummary {
    create_pdf(sources, dest, CreateOptions { size: PageSize::A4, pdf_a }).expect("create succeeds")
}

fn load(path: &Path) -> Document {
    Document::load_mem(&std::fs::read(path).expect("read")).expect("parse")
}

fn dict<'d>(doc: &'d Document, object: &'d Object) -> &'d Dictionary {
    match object {
        Object::Reference(id) => doc.get_object(*id).and_then(Object::as_dict).expect("a dictionary"),
        Object::Dictionary(d) => d,
        other => panic!("not a dictionary: {other:?}"),
    }
}

fn string(object: &Object) -> String {
    match object {
        Object::String(bytes, _) if bytes.starts_with(&[0xFE, 0xFF]) => {
            String::from_utf16(&bytes[2..].chunks_exact(2).map(|p| u16::from_be_bytes([p[0], p[1]])).collect::<Vec<_>>()).expect("UTF-16")
        }
        Object::String(bytes, _) => String::from_utf8_lossy(bytes).into_owned(),
        other => panic!("not a string: {other:?}"),
    }
}

/// The text between `<tag>` and `</tag>` in the XMP.
fn xmp_field<'x>(xmp: &'x str, tag: &str) -> &'x str {
    let start = xmp.find(&format!("<{tag}>")).unwrap_or_else(|| panic!("no {tag} in the XMP")) + tag.len() + 2;
    let end = xmp[start..].find(&format!("</{tag}>")).expect("closed") + start;
    &xmp[start..end]
}

/// `D:20261020081457+00'00'` as XMP writes the same instant.
fn as_xmp_date(pdf_date: &str) -> String {
    let d = pdf_date.strip_prefix("D:").expect("a PDF date");
    format!("{}-{}-{}T{}:{}:{}+00:00", &d[0..4], &d[4..6], &d[6..8], &d[8..10], &d[10..12], &d[12..14])
}

/// A PNG of `w` x `h` pixels, half of them transparent.
fn png_with_alpha(w: u32, h: u32) -> Vec<u8> {
    let pixels: Vec<u8> = (0..w * h).flat_map(|i| if i % 2 == 0 { [200, 30, 30, 255] } else { [30, 30, 200, 0] }).collect();
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, w, h);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header().expect("header").write_image_data(&pixels).expect("data");
    }
    bytes
}

fn ghostscript() -> Option<&'static str> {
    ["/opt/homebrew/bin/gs", "/usr/local/bin/gs", "gs"]
        .into_iter()
        .find(|p| Command::new(p).arg("--version").output().is_ok_and(|o| o.status.success()))
}

/// A red CMYK JPEG, which only Ghostscript among the tools here can write.
fn cmyk_jpeg(scratch: &Scratch) -> Option<PathBuf> {
    let gs = ghostscript()?;
    let path = scratch.path("cmyk.jpg");
    let status = Command::new(gs)
        .args(["-q", "-dNOPAUSE", "-dBATCH", "-sDEVICE=jpegcmyk", "-r72", "-g40x40"])
        .arg(format!("-sOutputFile={}", path.display()))
        .args(["-c", "1 0 0 setrgbcolor 0 0 40 40 rectfill showpage"])
        .status()
        .expect("runs gs");
    status.success().then_some(path)
}

/// Every kind of source at once: an image with transparency, plain text, a
/// Word document and an HTML page we didn't write, and (with Ghostscript) a
/// CMYK JPEG.
fn every_source(scratch: &Scratch) -> Vec<PathBuf> {
    let mut sources = vec![
        scratch.write("alpha.png", &png_with_alpha(32, 16)),
        scratch.write("notes.txt", "Archive note — naïve café, Øresund, Счёт 4821.\n".as_bytes()),
        PathBuf::from("../tests/fixtures/basic/textutil-memo.docx"),
        PathBuf::from("../tests/fixtures/basic/textutil-table.cocoa.html"),
    ];
    match cmyk_jpeg(scratch) {
        Some(jpeg) => {
            // Once as a page of its own, once inside a page that lays it out.
            sources.push(jpeg);
            sources.push(scratch.write("photo.html", b"<p>A photo:</p><img src=\"cmyk.jpg\" width=\"40\">"));
        }
        None => eprintln!("note: Ghostscript isn't installed, so the CMYK JPEG is left out"),
    }
    sources
}

/// veraPDF's executable, or `None` (with a note) when it isn't installed. CI
/// sets `VIBEPDF_REQUIRE_VERAPDF`, so there a missing validator fails.
fn verapdf() -> Option<PathBuf> {
    let found = ["/opt/homebrew/bin/verapdf", "/usr/local/bin/verapdf", "verapdf"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| Command::new(p).arg("--version").output().is_ok_and(|o| o.status.success()));
    if found.is_none() {
        assert!(std::env::var_os("VIBEPDF_REQUIRE_VERAPDF").is_none(), "VIBEPDF_REQUIRE_VERAPDF is set, but veraPDF isn't installed");
        eprintln!("skipping the veraPDF check: veraPDF is not installed (brew install verapdf)");
    }
    found
}

/// veraPDF's verdict on `pdf` against PDF/A-2u, with the failed rules when
/// there are any.
fn validate(verapdf: &Path, pdf: &Path) -> (bool, String) {
    let output = Command::new(verapdf).args(["--flavour", "2u", "--format", "mrr"]).arg(pdf).output().expect("runs veraPDF");
    let report = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(report.contains("isCompliant="), "veraPDF gave no verdict:\n{report}\n{}", String::from_utf8_lossy(&output.stderr));
    let compliant = report.contains("isCompliant=\"true\"");
    let failures: Vec<&str> = report.lines().filter(|l| l.contains("<rule ") && l.contains("status=\"failed\"") || l.contains("<description>")).collect();
    (compliant, failures.join("\n"))
}

// --- the file's structure -------------------------------------------------------

#[test]
fn every_source_makes_a_pdfa_file_with_its_parts() {
    let scratch = Scratch::new("parts");
    let dest = scratch.path("Quarterly archive.pdf");
    let summary = create(&every_source(&scratch), &dest, true);
    let doc = load(&dest);
    let catalog = doc.catalog().expect("catalog");

    // The output intent and the ICC's sRGB profile, byte for byte.
    let intents = catalog.get(b"OutputIntents").and_then(Object::as_array).expect("/OutputIntents");
    assert_eq!(intents.len(), 1);
    let intent = dict(&doc, &intents[0]);
    assert_eq!(intent.get(b"S").and_then(Object::as_name).expect("/S"), b"GTS_PDFA1");
    let profile = intent.get(b"DestOutputProfile").and_then(Object::as_reference).and_then(|r| doc.get_object(r)).and_then(Object::as_stream).expect("profile");
    assert_eq!(profile.dict.get(b"N").and_then(Object::as_i64).expect("/N"), 3);
    let bundled = std::fs::read("resources/icc/sRGB2014.icc").expect("the bundled profile");
    assert_eq!(profile.decompressed_content().unwrap_or_else(|_| profile.content.clone()), bundled);

    // Uncompressed XMP claiming PDF/A-2u, and Info entries that say the same.
    let metadata = catalog.get(b"Metadata").and_then(Object::as_reference).and_then(|r| doc.get_object(r)).and_then(Object::as_stream).expect("/Metadata");
    assert!(metadata.dict.get(b"Filter").is_err(), "PDF/A's metadata stream is never compressed");
    let xmp = String::from_utf8(metadata.content.clone()).expect("UTF-8 XMP");
    assert_eq!(xmp_field(&xmp, "pdfaid:part"), "2");
    assert_eq!(xmp_field(&xmp, "pdfaid:conformance"), "U");
    let info = dict(&doc, doc.trailer.get(b"Info").expect("/Info"));
    let title = string(info.get(b"Title").expect("/Title"));
    assert_eq!(title, "Quarterly archive", "titled after the file");
    assert!(xmp.contains(&format!(">{title}</rdf:li>")), "the XMP title");
    assert_eq!(xmp_field(&xmp, "pdf:Producer"), string(info.get(b"Producer").expect("/Producer")));
    assert_eq!(xmp_field(&xmp, "xmp:CreatorTool"), string(info.get(b"Creator").expect("/Creator")));
    assert_eq!(xmp_field(&xmp, "xmp:CreateDate"), as_xmp_date(&string(info.get(b"CreationDate").expect("date"))));
    assert_eq!(xmp_field(&xmp, "xmp:ModifyDate"), as_xmp_date(&string(info.get(b"ModDate").expect("date"))));

    // An identifier, no encryption.
    let id = doc.trailer.get(b"ID").and_then(Object::as_array).expect("/ID");
    assert_eq!(id.len(), 2);
    assert!(doc.trailer.get(b"Encrypt").is_err());

    // No DeviceCMYK anywhere; every font embedded and mapped to Unicode.
    let mut fonts = 0;
    for object in doc.objects.values() {
        let d = match object {
            Object::Dictionary(d) => d,
            Object::Stream(s) => &s.dict,
            _ => continue,
        };
        assert!(!d.get(b"ColorSpace").and_then(Object::as_name).is_ok_and(|n| n == b"DeviceCMYK"), "a DeviceCMYK image");
        if d.get(b"Type").and_then(Object::as_name).is_ok_and(|t| t == b"FontDescriptor") {
            assert!(d.has(b"FontFile2"), "a font isn't embedded");
        }
        if d.get(b"Subtype").and_then(Object::as_name).is_ok_and(|t| t == b"Type0") {
            fonts += 1;
            assert!(d.has(b"ToUnicode"), "a font has no Unicode map");
        }
    }
    assert!(fonts >= 3, "{fonts} fonts");
    assert!(summary.pages >= 4, "{summary:?}");
    if ghostscript().is_some() {
        let conversions = summary.warnings.iter().filter(|w| w.contains("converted to RGB")).count();
        assert_eq!(conversions, 2, "the JPEG on its own page and in the HTML page: {:?}", summary.warnings);
    }
}

#[test]
fn a_cmyk_jpeg_is_converted_to_rgb_and_keeps_its_colour() {
    let scratch = Scratch::new("cmyk");
    let Some(jpeg) = cmyk_jpeg(&scratch) else {
        eprintln!("skipping: Ghostscript is not installed");
        return;
    };
    let dest = scratch.path("out.pdf");
    let summary = create(std::slice::from_ref(&jpeg), &dest, true);
    assert!(summary.warnings.iter().any(|w| w.contains("converted to RGB")), "{:?}", summary.warnings);
    let doc = load(&dest);
    let image = doc.objects.values().filter_map(|o| o.as_stream().ok()).find(|s| s.dict.get(b"Subtype").and_then(Object::as_name).is_ok_and(|n| n == b"Image")).expect("an image");
    assert_eq!(image.dict.get(b"ColorSpace").and_then(Object::as_name).expect("space"), b"DeviceRGB");
    assert!(!image.dict.get(b"Filter").and_then(Object::as_name).is_ok_and(|f| f == b"DCTDecode"), "the JPEG was copied in");
    let (pdf, _) = open_pdf(&dest, None).expect("opens");
    let page = render_page(&pdf, 0, 72.0, ImageFormat::Rgba8).expect("render");
    let at = |x: u32, y: u32| {
        let i = usize::try_from((y * page.width + x) * 4).expect("index");
        [page.bytes[i], page.bytes[i + 1], page.bytes[i + 2]]
    };
    let middle = at(page.width / 2, page.height / 2);
    assert!(middle[0] > 180 && middle[1] < 90 && middle[2] < 90, "red came out as {middle:?}");

    // Without PDF/A, the same JPEG still goes in byte for byte.
    let plain = scratch.path("plain.pdf");
    let summary = create(&[jpeg], &plain, false);
    assert!(summary.warnings.is_empty(), "{:?}", summary.warnings);
    let doc = load(&plain);
    assert!(doc.objects.values().filter_map(|o| o.as_stream().ok()).any(|s| s.dict.get(b"ColorSpace").and_then(Object::as_name).is_ok_and(|n| n == b"DeviceCMYK")));
}

#[test]
fn without_the_option_nothing_changes() {
    let scratch = Scratch::new("plain");
    let dest = scratch.path("out.pdf");
    create(&[PathBuf::from("../tests/fixtures/basic/textutil-memo.docx")], &dest, false);
    let doc = load(&dest);
    let catalog = doc.catalog().expect("catalog");
    assert!(catalog.get(b"OutputIntents").is_err() && catalog.get(b"Metadata").is_err());
    let info = dict(&doc, doc.trailer.get(b"Info").expect("/Info"));
    assert!(info.get(b"Title").is_err() && info.get(b"CreationDate").is_err());
}

#[test]
fn a_character_no_font_has_is_refused_by_name() {
    let scratch = Scratch::new("notdef");
    // A private-use code point in plane 16: no installed face has a glyph for it.
    let page = scratch.write("box.html", "<p>Box \u{10FFFD} here</p>".as_bytes());
    let dest = scratch.path("out.pdf");
    match create_pdf(std::slice::from_ref(&page), &dest, CreateOptions { size: PageSize::A4, pdf_a: true }) {
        Err(CommandError::InvalidInput(m)) => assert!(m.contains("U+10FFFD") && m.contains("PDF/A"), "{m}"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(!dest.exists());
    // Outside PDF/A it's drawn as a box, and said.
    let summary = create(&[page], &dest, false);
    assert!(summary.warnings.iter().any(|w| w.contains("U+10FFFD") && w.contains("box")), "{:?}", summary.warnings);
}

#[test]
fn the_built_bytes_are_pdfa_before_the_save_path() {
    // The save path re-serializes through PDFium, which happens to keep all of
    // this (and adds an /ID of its own when there is none). The builder must
    // not depend on that: its own bytes carry the identifier and pass veraPDF.
    let scratch = Scratch::new("built");
    let (bytes, _) = build_pdf_with(&every_source(&scratch), CreateOptions { size: PageSize::A4, pdf_a: true }, "Built").expect("build");
    let doc = Document::load_mem(&bytes).expect("parse");
    let id = doc.trailer.get(b"ID").and_then(Object::as_array).expect("the builder's own /ID");
    assert_eq!(id.len(), 2);
    assert_eq!(id[0], id[1], "a new file's two identifiers are the same");
    let Some(verapdf) = verapdf() else { return };
    let path = scratch.write("built.pdf", &bytes);
    let (compliant, failures) = validate(&verapdf, &path);
    assert!(compliant, "the builder's bytes aren't PDF/A on their own:\n{failures}");
}

// --- conformance, by an outside validator ----------------------------------------------

#[test]
fn verapdf_accepts_every_source_as_pdfa_2u() {
    let Some(verapdf) = verapdf() else { return };
    let scratch = Scratch::new("verapdf");
    let dest = scratch.path("archive.pdf");
    create(&every_source(&scratch), &dest, true);
    let (compliant, failures) = validate(&verapdf, &dest);
    assert!(compliant, "veraPDF rejects it:\n{failures}");
}

#[test]
fn verapdf_rejects_a_file_made_without_the_option() {
    // The validator's own control: the same sources without PDF/A must fail,
    // or the passing test above proves nothing.
    let Some(verapdf) = verapdf() else { return };
    let scratch = Scratch::new("verapdf-control");
    let dest = scratch.path("plain.pdf");
    create(&every_source(&scratch), &dest, false);
    let (compliant, _) = validate(&verapdf, &dest);
    assert!(!compliant, "veraPDF accepts a file with no PDF/A metadata");
}

/// `Sample PDFs/out/p7-create/pdfa-*.pdf`, for Preview and veraPDF.
/// `cargo test --test create_pdfa -- --ignored --nocapture`
#[test]
#[ignore = "writes verification files for a human to open"]
fn writes_the_verification_files() {
    let dir = PathBuf::from("../Sample PDFs/out/p7-create");
    std::fs::create_dir_all(&dir).expect("dir");
    let scratch = Scratch::new("verify");
    let dest = dir.join("pdfa-every-source.pdf");
    let summary = create(&every_source(&scratch), &dest, true);
    println!("VERIFY pdfa-every-source.pdf: {summary:?}");
    let html = dir.join("html-features.html");
    if html.exists() {
        let summary = create(&[html], &dir.join("pdfa-html-features.pdf"), true);
        println!("VERIFY pdfa-html-features.pdf: {summary:?}");
    }
}
