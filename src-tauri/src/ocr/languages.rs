//! SPEC: P7-OCR-002 — which OCR languages exist, which are installed, and how
//! another one gets onto the machine.
//!
//! Twelve languages ship with the application (the list the spec names), fetched
//! at build time by `scripts/fetch-tessdata.sh` into `resources/tessdata/` and
//! bundled. They work with no network, ever.
//!
//! Anything beyond those twelve arrives one of two ways:
//!
//! - **From a file** the user already has. No network, always available.
//! - **By download**, which is off until the user turns it on
//!   ([`downloads_allowed`]). `docs/01_VISION.md` promises an offline-first
//!   application, and `CLAUDE.md` requires every network path to be explicitly
//!   enabled and to degrade gracefully, so this one is opt-in, user-initiated,
//!   and never a background fetch.
//!
//! Both paths end in [`install_pack`], which refuses anything Tesseract itself
//! cannot load. A "language pack" that does not work is worse than a missing
//! one: it would fail later, inside a document, with no obvious cause.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::CommandError;
use crate::ocr::engine::OcrEngine;
use crate::ocr::tessdata;
use crate::settings::{read_json, write_atomic};

/// The languages a build ships, as `(Tesseract code, English name)`.
/// SPEC: P7-OCR-002 names exactly these.
pub const BUNDLED: &[(&str, &str)] = &[
    ("eng", "English"),
    ("spa", "Spanish"),
    ("fra", "French"),
    ("deu", "German"),
    ("chi_sim", "Chinese (Simplified)"),
    ("chi_tra", "Chinese (Traditional)"),
    ("jpn", "Japanese"),
    ("kor", "Korean"),
    ("ara", "Arabic"),
    ("hin", "Hindi"),
    ("por", "Portuguese"),
    ("rus", "Russian"),
];

/// Where a pack came from, which decides whether it can be removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PackSource {
    /// Shipped with the application.
    Bundled,
    /// Installed by the user, in the app's data directory.
    Added,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguagePack {
    pub code: String,
    /// English name for the twelve we ship; otherwise just the code, because
    /// guessing a name for an arbitrary pack would be inventing information.
    pub name: String,
    pub source: PackSource,
    pub size_bytes: u64,
}

/// Every usable language, bundled and added, sorted by code.
#[must_use]
pub fn list(user_dir: &Path) -> Vec<LanguagePack> {
    let named = |code: &str| {
        BUNDLED
            .iter()
            .find(|(c, _)| *c == code)
            .map_or_else(|| code.to_owned(), |(_, name)| (*name).to_owned())
    };

    let mut packs: Vec<LanguagePack> = Vec::new();
    for code in tessdata::installed_languages() {
        let size = tessdata::directory_for(&code)
            .ok()
            .and_then(|dir| std::fs::metadata(dir.join(format!("{code}.traineddata"))).ok())
            .map_or(0, |m| m.len());
        packs.push(LanguagePack {
            name: named(&code),
            code,
            source: PackSource::Bundled,
            size_bytes: size,
        });
    }
    if let Ok(entries) = std::fs::read_dir(user_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(code) = name.strip_suffix(".traineddata") else {
                continue;
            };
            if packs.iter().any(|p| p.code == code) {
                continue;
            }
            packs.push(LanguagePack {
                code: code.to_owned(),
                name: named(code),
                source: PackSource::Added,
                size_bytes: entry.metadata().map_or(0, |m| m.len()),
            });
        }
    }
    packs.sort_by(|a, b| a.code.cmp(&b.code));
    packs
}

/// Install `bytes` as `code`'s language data under `user_dir`.
///
/// # Errors
/// [`CommandError::InvalidInput`] when the code is not a plausible Tesseract
/// language name, or when Tesseract refuses to load the data — which is the
/// only check that actually means anything. A file that is the right size and
/// the wrong contents fails here rather than inside someone's document.
pub fn install_pack(user_dir: &Path, code: &str, bytes: &[u8]) -> Result<LanguagePack, CommandError> {
    if code.is_empty()
        || code.len() > 32
        || !code.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(CommandError::InvalidInput(format!(
            "\"{code}\" is not a language code (letters, digits and underscore)"
        )));
    }
    if bytes.is_empty() {
        return Err(CommandError::InvalidInput(format!("{code}: the file is empty")));
    }

    // Prove it loads before it is installed, in a directory of its own so a
    // failed attempt leaves nothing behind.
    let staging = user_dir.join(format!(".staging-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&staging)?;
    let staged = staging.join(format!("{code}.traineddata"));
    std::fs::write(&staged, bytes)?;
    let usable = OcrEngine::with_datapath(&staging, code);
    if usable.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(CommandError::InvalidInput(format!(
            "{code}: Tesseract could not load this file as language data"
        )));
    }

    std::fs::create_dir_all(user_dir)?;
    let target = user_dir.join(format!("{code}.traineddata"));
    std::fs::rename(&staged, &target).or_else(|_| {
        // Rename can fail across filesystems; copying is the fallback.
        std::fs::copy(&staged, &target).map(|_| ())
    })?;
    let _ = std::fs::remove_dir_all(&staging);

    Ok(LanguagePack {
        code: code.to_owned(),
        name: BUNDLED
            .iter()
            .find(|(c, _)| *c == code)
            .map_or_else(|| code.to_owned(), |(_, name)| (*name).to_owned()),
        source: PackSource::Added,
        size_bytes: bytes.len() as u64,
    })
}

/// Read a `.traineddata` the user points at and install it.
///
/// # Errors
/// When the file can't be read, or [`install_pack`] rejects it.
pub fn install_from_file(user_dir: &Path, file: &Path) -> Result<LanguagePack, CommandError> {
    let code = file
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .ok_or_else(|| CommandError::InvalidInput("no file name".into()))?;
    let bytes = std::fs::read(file)?;
    install_pack(user_dir, &code, &bytes)
}

/// Remove an added pack. Bundled languages are part of the build and stay.
///
/// # Errors
/// [`CommandError::NotFound`] when no such pack was added.
pub fn remove_pack(user_dir: &Path, code: &str) -> Result<(), CommandError> {
    let target = user_dir.join(format!("{code}.traineddata"));
    if !target.is_file() {
        return Err(CommandError::NotFound(format!(
            "{code} is not an added language pack"
        )));
    }
    std::fs::remove_file(target)?;
    Ok(())
}

/// Where the download setting lives, under the app's config directory.
fn setting_file(config_dir: &Path) -> PathBuf {
    config_dir.join("ocr.json")
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct OcrSettings {
    version: u32,
    allow_downloads: bool,
}

/// Whether the user has turned language-pack downloads on. Default: no.
#[must_use]
pub fn downloads_allowed(config_dir: &Path) -> bool {
    read_json::<OcrSettings>(&setting_file(config_dir)).is_some_and(|s| s.allow_downloads)
}

/// Turn downloads on or off.
///
/// # Errors
/// When the setting cannot be written.
pub fn set_downloads_allowed(config_dir: &Path, allowed: bool) -> Result<(), CommandError> {
    let body = serde_json::to_vec_pretty(&OcrSettings {
        version: 1,
        allow_downloads: allowed,
    })
    .map_err(|e| CommandError::Internal(format!("could not write the OCR setting: {e}")))?;
    write_atomic(&setting_file(config_dir), &body)
}

/// The pinned snapshot downloads come from. A commit, not a branch: its
/// contents cannot change under us, which is the same guarantee
/// `scripts/fetch-tessdata.sh` gets from a checksum for the bundled files.
const TESSDATA_COMMIT: &str = "87416418657359cb625c412a48b6e1d6d41c29bd";

fn download_url(code: &str) -> String {
    format!(
        "https://raw.githubusercontent.com/tesseract-ocr/tessdata_fast/{TESSDATA_COMMIT}/{code}.traineddata"
    )
}

/// SPEC: P7-OCR-002 — download a language pack, if the user has allowed it.
///
/// The only network call in the application, and it happens because someone
/// asked for this language, in this moment. Refuses loudly when downloads are
/// off, rather than quietly doing nothing.
///
/// # Errors
/// [`CommandError::PermissionDenied`] when downloads are not enabled;
/// [`CommandError::NotFound`] when the pinned snapshot has no such language;
/// [`CommandError::IoError`] when the network fails — including when there is
/// no network, which must read as "couldn't fetch", not as a crash.
pub fn download_pack(
    user_dir: &Path,
    config_dir: &Path,
    code: &str,
) -> Result<LanguagePack, CommandError> {
    use std::io::Read as _;
    // Capped: a language model is a few megabytes, and an unbounded read from
    // the network is how a download turns into an out-of-memory crash.
    const MAX_PACK_BYTES: u64 = 64 * 1024 * 1024;

    if !downloads_allowed(config_dir) {
        return Err(CommandError::PermissionDenied(
            "downloading language packs is switched off. Turn it on in settings, or install a \
             .traineddata file you already have."
                .into(),
        ));
    }

    let response = ureq::get(&download_url(code)).call().map_err(|e| match e {
        ureq::Error::StatusCode(404) => {
            CommandError::NotFound(format!("no language pack called \"{code}\""))
        }
        other => CommandError::IoError(format!("could not download {code}: {other}")),
    })?;

    let mut bytes = Vec::new();
    response
        .into_body()
        .into_reader()
        .take(MAX_PACK_BYTES)
        .read_to_end(&mut bytes)
        .map_err(|e| CommandError::IoError(format!("could not read {code}: {e}")))?;

    install_pack(user_dir, code, &bytes)
}

#[allow(clippy::expect_used, clippy::unwrap_used)]
#[cfg(test)]
mod language_tests {
    use super::{downloads_allowed, set_downloads_allowed, BUNDLED};

    #[test]
    fn the_spec_languages_are_all_listed() {
        // SPEC: P7-OCR-002 names twelve; the list is what the fetch script and
        // the installer both read, so a typo here ships a missing language.
        assert_eq!(BUNDLED.len(), 12);
        for code in ["eng", "spa", "fra", "deu", "chi_sim", "chi_tra", "jpn", "kor", "ara", "hin", "por", "rus"] {
            assert!(BUNDLED.iter().any(|(c, _)| *c == code), "{code} missing");
        }
    }

    #[test]
    fn downloads_are_off_until_someone_turns_them_on() {
        let dir = std::env::temp_dir().join(format!("vibepdf-ocr-cfg-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        assert!(!downloads_allowed(&dir), "downloads must default to off");

        set_downloads_allowed(&dir, true).expect("write setting");
        assert!(downloads_allowed(&dir));
        set_downloads_allowed(&dir, false).expect("write setting");
        assert!(!downloads_allowed(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
