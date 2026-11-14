//! SPEC: NFR-PKG-001 — the application finds the files it ships with in the
//! folder recorded for it, the way a packaged copy does.
//!
//! A test binary has no such folder and normally falls back to this checkout.
//! Here one is built in a temp directory, in the shape Tauri gives a bundle
//! (`<folder>/resources/pdfium`, `<folder>/resources/tessdata`), and recorded
//! before anything loads.
//!
//! That checks our lookup against our idea of the shape. Whether a real bundle
//! has that shape is a separate question, answered in CI by `--self-check` on
//! a built `.app` and on an installed `.deb`.
//!
//! The folder is recorded once per process, so every test here shares it.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use pdfium_render::prelude::Pdfium;
use vibepdf_lib::bundled;
use vibepdf_lib::error::CommandError;
use vibepdf_lib::ocr::tessdata;
use vibepdf_lib::pdf::document::{pdfium, pdfium_library_path};

/// A language code no checkout has. Finding its data therefore proves the
/// recorded folder was searched, and not only the checkout behind it.
const ONLY_IN_THE_APP_FOLDER: &str = "zz_app_only";

fn checkout_resources() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("resources")
}

static APP_FOLDER: LazyLock<PathBuf> = LazyLock::new(|| {
    let app = std::env::temp_dir()
        .join(format!("vibepdf-app-{}", uuid::Uuid::new_v4()))
        .join("Resources");
    let engine = app.join("resources").join("pdfium");
    let ocr = app.join("resources").join("tessdata");
    std::fs::create_dir_all(&engine).unwrap();
    std::fs::create_dir_all(&ocr).unwrap();

    // A link where the filesystem allows one: this folder outlives the test
    // (the library stays loaded), and a 7 MB copy per run would pile up.
    let name = Pdfium::pdfium_platform_library_name();
    let pinned = checkout_resources().join("pdfium").join(&name);
    assert!(pinned.is_file(), "run `npm run fetch-pdfium`");
    if std::fs::hard_link(&pinned, engine.join(&name)).is_err() {
        std::fs::copy(&pinned, engine.join(&name)).unwrap();
    }
    std::fs::write(ocr.join(format!("{ONLY_IN_THE_APP_FOLDER}.traineddata")), b"").unwrap();

    bundled::set_root(app.clone());
    app
});

#[test]
fn the_app_folder_is_searched_before_this_checkout() {
    let app = &*APP_FOLDER;
    assert_eq!(bundled::root(), Some(app.as_path()));
    for name in ["pdfium", "tessdata"] {
        assert_eq!(
            bundled::folders(name),
            [app.join("resources").join(name), checkout_resources().join(name)],
        );
    }
}

#[test]
fn pdfium_loads_from_the_app_folder_and_opens_hello_pdf() {
    let app = &*APP_FOLDER;
    let pdfium = pdfium().expect("the PDF engine in the app folder should load");

    // The checkout holds the same library, so only the recorded path tells
    // the two apart.
    let expected = app
        .join("resources")
        .join("pdfium")
        .join(Pdfium::pdfium_platform_library_name());
    assert_eq!(pdfium_library_path(), Some(expected.as_path()));

    // Test runs with CWD = src-tauri/.
    let doc = pdfium
        .load_pdf_from_file("../tests/fixtures/basic/hello.pdf", None)
        .expect("hello.pdf failed to load");
    assert_eq!(doc.pages().len(), 1);
}

#[test]
fn ocr_data_is_found_in_the_app_folder() {
    let app = &*APP_FOLDER;
    assert_eq!(
        tessdata::directory_for(ONLY_IN_THE_APP_FOLDER).unwrap(),
        app.join("resources").join("tessdata"),
    );
    assert!(tessdata::installed_languages().contains(&ONLY_IN_THE_APP_FOLDER.to_owned()));
}

// Why `--self-check` asks *where* a language was found, and not only whether:
// in a debug build one that the app folder lacks still turns up, from the
// checkout. A bundle with its data in the wrong place would look fine on the
// machine that built it.
#[test]
fn in_a_debug_build_a_language_the_app_folder_lacks_comes_from_the_checkout() {
    let _ = &*APP_FOLDER;
    let found = tessdata::directory_for("eng").expect("run `npm run fetch-tessdata`");
    if std::env::var_os(tessdata::TESSDATA_ENV).is_none() {
        assert_eq!(found, checkout_resources().join("tessdata"));
    }
}

#[test]
fn a_language_nobody_has_is_refused_and_the_app_folder_is_named_first() {
    let app = &*APP_FOLDER;
    let Err(CommandError::NotFound(message)) = tessdata::directory_for("zz_nobody") else {
        panic!("a language with no data anywhere was found");
    };
    let in_app = app.join("resources").join("tessdata").display().to_string();
    let in_checkout = checkout_resources().join("tessdata").display().to_string();
    let (at_app, at_checkout) = (
        message.find(&in_app).expect("the app folder is named"),
        message.find(&in_checkout).expect("the checkout is named"),
    );
    assert!(at_app < at_checkout, "{message}");
}
