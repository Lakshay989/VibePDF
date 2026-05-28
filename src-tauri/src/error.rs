use serde::Serialize;
use thiserror::Error;

/// Typed error surface for every Tauri command. The frontend pattern-
/// matches on `code` to choose the UI affordance (see `src/ipc/invoke.ts`).
#[derive(Debug, Error)]
pub enum CommandError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    /// SPEC: P1-VIEW-003 — emitted when an encrypted PDF is opened
    /// without a password, or with the wrong password. `PDFium` does
    /// not distinguish those two cases (both surface as
    /// `PdfiumInternalError::PasswordError`), and the spec doesn't
    /// need us to. Payload carries the document path so the UI can
    /// re-prompt against the same file.
    #[error("password required: {0}")]
    PasswordRequired(String),
    #[error("pdf error: {0}")]
    PdfError(String),
    #[error("io error: {0}")]
    IoError(String),
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("internal: {0}")]
    Internal(String),
}

#[derive(Serialize)]
struct WireError<'a> {
    code: &'a str,
    message: String,
}

impl Serialize for CommandError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let (code, message) = match self {
            CommandError::NotFound(m) => ("NotFound", m.clone()),
            CommandError::InvalidInput(m) => ("InvalidInput", m.clone()),
            CommandError::PasswordRequired(m) => ("PasswordRequired", m.clone()),
            CommandError::PdfError(m) => ("PdfError", m.clone()),
            CommandError::IoError(m) => ("IoError", m.clone()),
            CommandError::PermissionDenied(m) => ("PermissionDenied", m.clone()),
            CommandError::Internal(m) => ("Internal", m.clone()),
        };
        WireError { code, message }.serialize(s)
    }
}

impl From<std::io::Error> for CommandError {
    fn from(e: std::io::Error) -> Self {
        CommandError::IoError(e.to_string())
    }
}

impl From<pdfium_render::prelude::PdfiumError> for CommandError {
    fn from(e: pdfium_render::prelude::PdfiumError) -> Self {
        // SPEC: P1-VIEW-003 — pdfium's `PasswordError` (`FPDF_ERR_PASSWORD`,
        // raised both for "no password supplied for an encrypted file"
        // and "wrong password") gets its own typed variant so the
        // frontend can mount the prompt dialog. Every other pdfium
        // failure remains a generic `PdfError` — including
        // `SecurityError` (unsupported security scheme), which is *not*
        // a wrong-password and a re-prompt would not help.
        use pdfium_render::prelude::{PdfiumError, PdfiumInternalError};
        match e {
            PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => {
                CommandError::PasswordRequired(String::new())
            }
            other => CommandError::PdfError(other.to_string()),
        }
    }
}

impl From<anyhow::Error> for CommandError {
    fn from(e: anyhow::Error) -> Self {
        CommandError::Internal(e.to_string())
    }
}
