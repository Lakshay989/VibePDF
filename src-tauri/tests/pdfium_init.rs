//! Smoke test: PDFium native library is loadable and can open the
//! minimal hello.pdf fixture.
//!
//! This test deliberately mirrors the only thing the Rust side does in
//! Phase 1 — open a PDF and read its page count. If this test passes,
//! `bind_to_system_library`/`bind_to_statically_linked_library` resolved
//! the prebuilt PDFium binary and `pdfium-render` is wired correctly.

use pdfium_render::prelude::*;

#[test]
fn pdfium_can_open_hello_pdf() {
    let bindings = Pdfium::bind_to_statically_linked_library()
        .or_else(|_| Pdfium::bind_to_system_library())
        .expect("could not load PDFium native library — run `npm run fetch-pdfium`");
    let pdfium = Pdfium::new(bindings);

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
