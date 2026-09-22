//! SPEC: P7-OCR-002 — IPC surface for OCR language packs.
//!
//! The only callers that know where added packs live. They resolve
//! `app_data_dir()` / `app_config_dir()` through the Tauri `AppHandle` and
//! delegate to `ocr::languages`, which is `AppHandle`-free and unit-tested
//! against temp directories.
//!
//! Downloading is a deliberate, user-initiated act: `ocr_download_language`
//! refuses unless `ocr_set_downloads_allowed(true)` has been called, and that
//! setting persists as off until someone changes it.

use std::path::PathBuf;

use tauri::{AppHandle, Manager};

use crate::error::CommandError;
use crate::ocr::languages::{self, LanguagePack};

/// `<app_data_dir>/tessdata` — where added packs live, beside nothing else, so
/// listing it is unambiguous.
fn user_tessdata_dir(app: &AppHandle) -> Result<PathBuf, CommandError> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| CommandError::Internal(format!("app_data_dir unavailable: {e}")))?;
    Ok(dir.join("tessdata"))
}

fn config_dir(app: &AppHandle) -> Result<PathBuf, CommandError> {
    app.path()
        .app_config_dir()
        .map_err(|e| CommandError::Internal(format!("app_config_dir unavailable: {e}")))
}

/// SPEC: P7-OCR-002 — every language OCR can use, bundled or added.
#[tauri::command]
pub async fn ocr_list_languages(app: AppHandle) -> Result<Vec<LanguagePack>, CommandError> {
    Ok(languages::list(&user_tessdata_dir(&app)?))
}

/// SPEC: P7-OCR-002 — install a `.traineddata` the user already has. Always
/// available; needs no network.
#[tauri::command]
pub async fn ocr_install_language_file(
    app: AppHandle,
    path: String,
) -> Result<LanguagePack, CommandError> {
    languages::install_from_file(&user_tessdata_dir(&app)?, std::path::Path::new(&path))
}

/// SPEC: P7-OCR-002 — fetch a language pack. Refused unless downloads have
/// been switched on.
#[tauri::command]
pub async fn ocr_download_language(
    app: AppHandle,
    code: String,
) -> Result<LanguagePack, CommandError> {
    let (user_dir, config) = (user_tessdata_dir(&app)?, config_dir(&app)?);
    // Blocking network IO off the async runtime's threads.
    tauri::async_runtime::spawn_blocking(move || {
        languages::download_pack(&user_dir, &config, &code)
    })
    .await
    .map_err(|e| CommandError::Internal(format!("download task failed: {e}")))?
}

/// Remove an added pack. Bundled languages are part of the build and stay.
#[tauri::command]
pub async fn ocr_remove_language(app: AppHandle, code: String) -> Result<(), CommandError> {
    languages::remove_pack(&user_tessdata_dir(&app)?, &code)
}

/// Whether language-pack downloads are switched on. Off until set.
#[tauri::command]
pub async fn ocr_downloads_allowed(app: AppHandle) -> Result<bool, CommandError> {
    Ok(languages::downloads_allowed(&config_dir(&app)?))
}

/// Switch language-pack downloads on or off. The only thing that enables the
/// application's single network path.
#[tauri::command]
pub async fn ocr_set_downloads_allowed(
    app: AppHandle,
    allowed: bool,
) -> Result<bool, CommandError> {
    languages::set_downloads_allowed(&config_dir(&app)?, allowed)?;
    Ok(allowed)
}
