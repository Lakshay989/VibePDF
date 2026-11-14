//! Smoke test: the PDF engine loads the way the application loads it, and
//! opens the minimal hello.pdf fixture.
//!
//! Until P7.HF5 this test bound `PDFium` itself, by bare library name, which
//! is what the application did too. The application now loads it by full path
//! (SPEC: NFR-PKG-001), so the test goes through the same call: a test that
//! kept its own loading would pass on a machine where the application's fails.

use vibepdf_lib::pdf::document::{pdfium, pdfium_library_path};

#[test]
fn pdfium_can_open_hello_pdf() {
    let pdfium = pdfium().expect("could not load PDFium — run `npm run fetch-pdfium`");

    // SPEC: NFR-PKG-001 — a test binary has no application folder, so the
    // engine must be this checkout's pinned copy and nothing found by search.
    let loaded = pdfium_library_path().expect("the engine loaded, so its path is recorded");
    let pinned = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("resources")
        .join("pdfium");
    assert_eq!(loaded.parent(), Some(pinned.as_path()));

    // Path is resolved relative to the workspace root from the test
    // runner's CWD (which is the crate dir, src-tauri/).
    let fixture = std::path::Path::new("../tests/fixtures/basic/hello.pdf");
    assert!(
        fixture.exists(),
        "fixture missing at {}",
        fixture.display()
    );

    let doc = pdfium
        .load_pdf_from_file(fixture, None)
        .expect("hello.pdf failed to load");
    assert_eq!(doc.pages().len(), 1);
}
