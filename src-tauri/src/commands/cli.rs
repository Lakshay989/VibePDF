//! SPEC: P1-VIEW-001 — the command-line-argument clause.
//!
//! `setup` parses `std::env::args()` once (see `lib.rs`), keeps the
//! `.pdf` paths that exist on disk, and buffers them in
//! `AppState.cli_pending`. The frontend drains the buffer on mount via
//! `cli_take_pending_opens` and routes each path through the normal
//! `openByPath` — so CLI files inherit recents (A3), session
//! persistence (E1), and the password prompt (B2) for free.
//!
//! ### Why a pull command, not a `cli-open` event
//!
//! `steps/P1.md` floats emitting a `cli-open` event in `setup`, but
//! `setup` runs before the webview / React mount, so the listener does
//! not exist yet and the event is silently dropped. A drain command
//! sidesteps the race entirely.

use std::path::{Path, PathBuf};

use tauri::State;

use crate::error::CommandError;
use crate::ocr::engine::OcrEngine;
use crate::ocr::{languages, tessdata};
use crate::AppState;

/// Filter `args` to PDF paths. Pure: no IO, just extension matching.
///
/// - Drops `argv[0]` (the binary path), matching standard CLI conventions.
/// - Keeps anything ending in `.pdf` (case-insensitive).
/// - Preserves input order.
///
/// Existence checking happens at the call site in `lib.rs::setup`, so
/// the pure logic stays trivially unit-testable in `tests/cli_open.rs`.
pub fn pdf_paths_from_args<I, S>(args: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    args.into_iter()
        .skip(1)
        .map(Into::into)
        .filter(|s| {
            // ends_with on a lowercased copy avoids allocating on every
            // non-pdf arg (.to_lowercase only fires when we have to).
            s.len() >= 4 && s[s.len() - 4..].eq_ignore_ascii_case(".pdf")
        })
        .collect()
}

/// SPEC: P1-VIEW-001 — drain the CLI-pending buffer. Returns the
/// paths once and clears them so a refresh / remount can't re-open
/// the same files.
#[tauri::command]
pub async fn cli_take_pending_opens(
    state: State<'_, AppState>,
) -> Result<Vec<String>, CommandError> {
    let mut guard = state
        .cli_pending
        .lock()
        .map_err(|e| CommandError::Internal(format!("cli_pending lock poisoned: {e}")))?;
    Ok(std::mem::take(&mut *guard))
}

/// The argument that asks for [`self_check`] instead of a window.
pub const SELF_CHECK_FLAG: &str = "--self-check";

/// Whether `args` ask for the self-check. `argv[0]` is skipped, as in
/// [`pdf_paths_from_args`].
pub fn self_check_requested<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    args.into_iter()
        .skip(1)
        .any(|arg| arg.as_ref() == SELF_CHECK_FLAG)
}

/// What [`self_check`] found: one line per thing checked, and the verdict.
pub struct SelfCheck {
    pub passed: bool,
    pub report: String,
}

/// SPEC: NFR-PKG-001 — does this copy of the application have everything it
/// ships with, inside its own folder?
///
/// It loads the PDF engine, finds each shipped OCR language the way the OCR
/// engine would, and starts that engine once. `run()` calls it before any
/// window exists, so it needs no display, and the user's added language packs
/// are not registered yet: only what the application itself carries is judged.
///
/// This is the check that sees a real bundle. A unit test of the lookup can
/// only compare our code with our own idea of where a bundle puts things.
#[must_use]
pub fn self_check() -> SelfCheck {
    let engine = crate::pdf::document::pdfium()
        .map_err(|e| e.to_string())
        .and_then(|_| {
            crate::pdf::document::pdfium_library_path()
                .map(Path::to_path_buf)
                .ok_or_else(|| "loaded, but from a path that was not recorded".to_owned())
        });
    let found: Vec<(&str, Result<PathBuf, String>)> = languages::BUNDLED
        .iter()
        .map(|(code, _)| (*code, tessdata::directory_for(code).map_err(|e| e.to_string())))
        .collect();
    let ocr_starts = OcrEngine::new(OCR_PROBE_LANGUAGE)
        .map(|_| ())
        .map_err(|e| e.to_string());
    judge(crate::bundled::root(), &engine, &found, &ocr_starts)
}

/// The language the self-check starts the OCR engine with: the one every
/// build has had since OCR shipped.
const OCR_PROBE_LANGUAGE: &str = "eng";

/// The verdict, from where things were found. Pure, so each way of failing
/// has a test.
fn judge(
    root: Option<&Path>,
    engine: &Result<PathBuf, String>,
    languages: &[(&str, Result<PathBuf, String>)],
    ocr_starts: &Result<(), String>,
) -> SelfCheck {
    let mut passed = true;
    let mut lines = vec!["VibePDF self-check".to_owned()];
    let mut note = |ok: bool, text: String| {
        passed &= ok;
        lines.push(format!("{} {text}", if ok { "ok  " } else { "FAIL" }));
    };
    // Compared by path component, so `Resources-other` is not inside `Resources`.
    let inside = |path: &Path| root.is_some_and(|root| path.starts_with(root));
    let located = |what: &str, found: &Result<PathBuf, String>| match found {
        Ok(path) if inside(path) => (true, format!("{what}: {}", path.display())),
        Ok(path) => (
            false,
            format!("{what}: found outside the application, at {}", path.display()),
        ),
        Err(why) => (false, format!("{what}: {why}")),
    };

    match root {
        Some(root) => note(true, format!("application folder: {}", root.display())),
        None => note(false, "application folder: not found".to_owned()),
    }
    let (ok, text) = located("PDF engine", engine);
    note(ok, text);
    for (code, found) in languages {
        let (ok, text) = located(&format!("OCR language {code}"), found);
        note(ok, text);
    }
    match ocr_starts {
        Ok(()) => note(true, format!("OCR engine: started with {OCR_PROBE_LANGUAGE}")),
        Err(why) => note(false, format!("OCR engine: {why}")),
    }

    lines.push(format!("result: {}", if passed { "PASS" } else { "FAIL" }));
    SelfCheck {
        passed,
        report: lines.join("\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Absolute on every platform, which a literal like `/app` is not on Windows.
    fn app() -> PathBuf {
        std::env::temp_dir().join("VibePDF.app").join("Resources")
    }

    fn engine_in(root: &Path) -> PathBuf {
        root.join("resources").join("pdfium").join("libpdfium.dylib")
    }

    fn languages_in(root: &Path) -> Vec<(&'static str, Result<PathBuf, String>)> {
        ["eng", "deu"]
            .into_iter()
            .map(|code| (code, Ok(root.join("resources").join("tessdata"))))
            .collect()
    }

    fn failures(check: &SelfCheck) -> Vec<&str> {
        check
            .report
            .lines()
            .filter(|line| line.starts_with("FAIL"))
            .collect()
    }

    // SPEC: NFR-PKG-001
    #[test]
    fn self_check_passes_when_everything_is_inside_the_app_folder() {
        let app = app();
        let check = judge(Some(&app), &Ok(engine_in(&app)), &languages_in(&app), &Ok(()));
        assert!(check.passed, "{}", check.report);
        assert!(failures(&check).is_empty(), "{}", check.report);
        assert!(check.report.ends_with("result: PASS"), "{}", check.report);
        // One line each: heading, folder, engine, two languages, OCR start, verdict.
        assert_eq!(check.report.lines().count(), 7, "{}", check.report);
    }

    // SPEC: NFR-PKG-001 — "only from its own installation folder".
    #[test]
    fn self_check_fails_when_the_library_is_outside_the_app_folder() {
        let app = app();
        let elsewhere = std::env::temp_dir().join("lib").join("libpdfium.dylib");
        let check = judge(Some(&app), &Ok(elsewhere), &languages_in(&app), &Ok(()));
        assert!(!check.passed);
        let failed = failures(&check);
        assert_eq!(failed.len(), 1, "{}", check.report);
        assert!(failed[0].contains("PDF engine: found outside the application"), "{}", failed[0]);
        assert!(check.report.ends_with("result: FAIL"), "{}", check.report);
    }

    // A folder whose name only starts the same way is a different folder.
    #[test]
    fn a_folder_that_only_shares_a_prefix_is_outside() {
        let app = app();
        let lookalike = app.with_file_name("Resources-other");
        let check = judge(Some(&app), &Ok(engine_in(&lookalike)), &languages_in(&app), &Ok(()));
        assert!(!check.passed, "{}", check.report);
        assert_eq!(failures(&check).len(), 1, "{}", check.report);
    }

    #[test]
    fn self_check_fails_when_the_engine_does_not_load() {
        let app = app();
        let missing = Err("the PDF engine is missing".to_owned());
        let check = judge(Some(&app), &missing, &languages_in(&app), &Ok(()));
        assert!(!check.passed);
        assert_eq!(failures(&check), ["FAIL PDF engine: the PDF engine is missing"]);
    }

    // SPEC: NFR-PKG-001 — "a bundled OCR language is missing".
    #[test]
    fn self_check_fails_when_a_shipped_language_is_missing() {
        let app = app();
        let mut languages = languages_in(&app);
        languages[1].1 = Err("no OCR language data for \"deu\"".to_owned());
        let check = judge(Some(&app), &Ok(engine_in(&app)), &languages, &Ok(()));
        assert!(!check.passed);
        assert_eq!(
            failures(&check),
            ["FAIL OCR language deu: no OCR language data for \"deu\""]
        );
    }

    // What a wrong lookup in a debug bundle looks like: the language is found,
    // but in the checkout the bundle was built from.
    #[test]
    fn self_check_fails_when_a_shipped_language_comes_from_outside_the_app_folder() {
        let app = app();
        let mut languages = languages_in(&app);
        languages[0].1 = Ok(std::env::temp_dir().join("src-tauri").join("resources").join("tessdata"));
        let check = judge(Some(&app), &Ok(engine_in(&app)), &languages, &Ok(()));
        assert!(!check.passed);
        let failed = failures(&check);
        assert_eq!(failed.len(), 1, "{}", check.report);
        assert!(failed[0].starts_with("FAIL OCR language eng: found outside the application"), "{}", failed[0]);
    }

    #[test]
    fn self_check_fails_without_an_app_folder() {
        let app = app();
        let check = judge(None, &Ok(engine_in(&app)), &languages_in(&app), &Ok(()));
        assert!(!check.passed);
        // Nothing can be inside a folder that is not known.
        assert_eq!(failures(&check).len(), 4, "{}", check.report);
        assert!(check.report.contains("FAIL application folder: not found"), "{}", check.report);
    }

    #[test]
    fn self_check_fails_when_the_ocr_engine_does_not_start() {
        let app = app();
        let refused = Err("could not start the OCR engine".to_owned());
        let check = judge(Some(&app), &Ok(engine_in(&app)), &languages_in(&app), &refused);
        assert!(!check.passed);
        assert_eq!(failures(&check), ["FAIL OCR engine: could not start the OCR engine"]);
    }

    #[test]
    fn the_flag_is_recognised_only_as_an_argument() {
        assert!(self_check_requested(["vibepdf", "--self-check"]));
        assert!(self_check_requested(["vibepdf", "a.pdf", "--self-check"]));
        assert!(!self_check_requested(["vibepdf"]));
        assert!(!self_check_requested(["vibepdf", "a.pdf"]));
        assert!(!self_check_requested(["vibepdf", "--self-check-later"]));
        // A binary that happens to be called like the flag does not ask for it.
        assert!(!self_check_requested(["--self-check"]));
    }
}
