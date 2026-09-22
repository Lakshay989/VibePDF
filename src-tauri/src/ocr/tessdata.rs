//! Where Tesseract's language data lives.
//!
//! The data is fetched by `scripts/fetch-tessdata.sh` into
//! `src-tauri/resources/tessdata/` and bundled by `tauri.conf.json`, the same
//! shape as `PDFium`'s binary: too big to commit, pinned by checksum, and copied
//! next to the executable at build time.
//!
//! Resolution order, first hit wins:
//! 1. `VIBEPDF_TESSDATA` — an explicit override, used by the test suite.
//! 2. The bundled `resources/tessdata` beside the executable (a real install).
//! 3. `src-tauri/resources/tessdata` relative to the crate (a dev build).
//!
//! There is deliberately no fallback to a system-wide Tesseract installation:
//! what a build ships is then the same everywhere, and a developer machine
//! can't pass a test that a user's machine would fail.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::error::CommandError;

/// Env var holding a directory of `*.traineddata` files.
pub const TESSDATA_ENV: &str = "VIBEPDF_TESSDATA";

/// Where packs the user added live, registered once at startup by the Tauri
/// setup hook (the engine has no `AppHandle`). Unset in tests, which point
/// [`TESSDATA_ENV`] at a temp directory instead.
static USER_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Register the directory added language packs are installed into.
/// Later calls are ignored: the path does not change while the app runs.
pub fn set_user_dir(dir: PathBuf) {
    let _ = USER_DIR.set(dir);
}

/// The directory to hand Tesseract, verified to hold `language`'s data.
///
/// # Errors
/// [`CommandError::NotFound`] when no candidate directory holds
/// `<language>.traineddata`, naming the fetch script — a missing download is
/// the overwhelmingly likely cause, and the message is what a contributor sees
/// first.
pub fn directory_for(language: &str) -> Result<PathBuf, CommandError> {
    let mut tried = Vec::new();
    for candidate in candidates() {
        if candidate.join(format!("{language}.traineddata")).is_file() {
            return Ok(candidate);
        }
        tried.push(candidate.display().to_string());
    }
    Err(CommandError::NotFound(format!(
        "no OCR language data for \"{language}\". Run `npm run fetch-tessdata` to download it. \
         Looked in: {}",
        tried.join(", ")
    )))
}

/// Languages whose data is present, sorted. Empty when nothing is installed.
#[must_use]
pub fn installed_languages() -> Vec<String> {
    let mut found: Vec<String> = candidates()
        .iter()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.strip_suffix(".traineddata").map(str::to_owned)
        })
        .collect();
    found.sort();
    found.dedup();
    found
}

fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(dir) = std::env::var(TESSDATA_ENV) {
        out.push(PathBuf::from(dir));
    }
    if let Some(dir) = USER_DIR.get() {
        out.push(dir.clone());
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            out.push(dir.join("resources").join("tessdata"));
            // macOS app bundle: Contents/MacOS/<exe> → Contents/Resources.
            if let Some(contents) = dir.parent() {
                out.push(contents.join("Resources").join("tessdata"));
            }
        }
    }
    out.push(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("tessdata"),
    );
    out
}
