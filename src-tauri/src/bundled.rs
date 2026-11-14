//! Where the files the application ships with are.
//!
//! Two things are not compiled into the binary: the `PDFium` library and
//! Tesseract's language data. `tauri.conf.json` bundles both from
//! `src-tauri/resources/` and keeps that path, so they sit at
//! `<resource folder>/resources/pdfium` and `<resource folder>/resources/tessdata`.
//! Which folder the resource folder is depends on the platform and on how the
//! application was started. Tauri knows, so `run()` asks it once and records
//! the answer here (docs/04 "Files the app ships with").
//!
//! SPEC: NFR-PKG-001 — a shipped application looks in its own folder and
//! nowhere else.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static ROOT: OnceLock<PathBuf> = OnceLock::new();

/// The checkout this was built from, for test binaries: nothing records a
/// resource folder for them. Debug builds only, so a release build has one
/// place to look and carries no path from the machine that built it.
#[cfg(debug_assertions)]
const SOURCE_CHECKOUT: Option<&str> = Some(env!("CARGO_MANIFEST_DIR"));
#[cfg(not(debug_assertions))]
const SOURCE_CHECKOUT: Option<&str> = None;

/// Record the application's resource folder. Called once, by `run()`, before
/// anything can ask for a bundled file. Later calls are ignored: the folder
/// does not move while the application runs.
pub fn set_root(dir: PathBuf) {
    let _ = ROOT.set(dir);
}

/// The recorded resource folder, if one has been recorded.
#[must_use]
pub fn root() -> Option<&'static Path> {
    ROOT.get().map(PathBuf::as_path)
}

/// The folders that may hold the bundled `name` (`"pdfium"`, `"tessdata"`),
/// the application's own first.
#[must_use]
pub fn folders(name: &str) -> Vec<PathBuf> {
    let source_tree = SOURCE_CHECKOUT.map(|checkout| Path::new(checkout).join("resources"));
    folders_from(root(), source_tree.as_deref(), name)
}

/// The path of `file` in the first of `folders` that holds it.
#[must_use]
pub fn first_holding(folders: &[PathBuf], file: &OsStr) -> Option<PathBuf> {
    folders
        .iter()
        .map(|folder| folder.join(file))
        .find(|path| path.is_file())
}

fn folders_from(root: Option<&Path>, source_tree: Option<&Path>, name: &str) -> Vec<PathBuf> {
    // A relative folder would be read against the current folder, which is
    // wherever the application was started from: the lookup this module
    // exists to rule out.
    root.filter(|root| root.is_absolute())
        .map(|root| root.join("resources"))
        .into_iter()
        .chain(source_tree.map(Path::to_path_buf))
        .map(|resources| resources.join(name))
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Absolute on every platform, which a literal like `/app` is not on Windows.
    fn base() -> PathBuf {
        std::env::temp_dir().join(format!("vibepdf-bundled-{}", uuid::Uuid::new_v4()))
    }

    // SPEC: NFR-PKG-001
    #[test]
    fn the_app_folder_is_tried_before_any_other() {
        let (app, checkout) = (base().join("app"), base().join("checkout"));
        assert_eq!(
            folders_from(Some(&app), Some(&checkout), "pdfium"),
            [
                app.join("resources").join("pdfium"),
                checkout.join("pdfium")
            ]
        );
    }

    // SPEC: NFR-PKG-001 — what a release build asks for: `source_tree` is
    // `None` there, so the application's own folder is the only answer.
    #[test]
    fn without_a_checkout_only_the_app_folder_is_offered() {
        let app = base();
        assert_eq!(
            folders_from(Some(&app), None, "tessdata"),
            [app.join("resources").join("tessdata")]
        );
        assert!(folders_from(None, None, "tessdata").is_empty());
    }

    #[test]
    fn a_test_binary_with_no_app_folder_gets_the_checkout() {
        let checkout = base();
        assert_eq!(
            folders_from(None, Some(&checkout), "pdfium"),
            [checkout.join("pdfium")]
        );
    }

    // SPEC: NFR-PKG-001 — "only from its own installation folder".
    #[test]
    fn a_relative_app_folder_is_ignored() {
        assert!(folders_from(Some(Path::new("Resources")), None, "pdfium").is_empty());
        assert!(folders_from(Some(Path::new("")), None, "pdfium").is_empty());
    }

    #[test]
    fn the_first_folder_holding_the_file_wins_and_a_folder_of_that_name_does_not_count() {
        let base = base();
        let (empty, decoy, first, second) = (
            base.join("empty"),
            base.join("decoy"),
            base.join("first"),
            base.join("second"),
        );
        for folder in [&empty, &first, &second] {
            std::fs::create_dir_all(folder).unwrap();
        }
        std::fs::create_dir_all(decoy.join("engine.lib")).unwrap();
        std::fs::write(first.join("engine.lib"), b"1").unwrap();
        std::fs::write(second.join("engine.lib"), b"2").unwrap();

        let name = OsStr::new("engine.lib");
        let all = [empty.clone(), decoy.clone(), first.clone(), second];
        assert_eq!(first_holding(&all, name), Some(first.join("engine.lib")));
        assert_eq!(first_holding(&[empty, decoy], name), None);
        assert_eq!(first_holding(&[], name), None);

        std::fs::remove_dir_all(&base).unwrap();
    }
}
