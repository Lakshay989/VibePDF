use std::ffi::c_void;
use std::os::raw::{c_int, c_ulong};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, MutexGuard, OnceLock};

use pdfium_render::prelude::*;
use serde::Serialize;

use crate::error::CommandError;

/// Process-global lock serializing **every** call into `PDFium` that
/// touches its shared mutable state: document load/save, metadata reads,
/// page lookups, page mutation (rotate, …), and rendering.
///
/// `PDFium` is documented as per-document-safe but NOT process-safe — two
/// threads each operating on their *own* document still race on global
/// subsystems (`FX_GE`, the page-state cache) and SIGABRT; even
/// `pages().get(idx)` reproduces it. The per-document actor serializes
/// *within* a document; this lock extends that across documents (the
/// autosave tick, or two open documents, can otherwise call in at once).
///
/// **Reentrancy:** the `Mutex` is not reentrant. Hold it around the
/// minimal FFI span and never across a call that re-locks — e.g.
/// `open_pdf` and `save_document` release before paths
/// (`verify_pdf_reopens`, the standalone `collect_metadata`) that re-lock.
pub(crate) static PDFIUM_LOCK: Mutex<()> = Mutex::new(());

/// Acquire [`PDFIUM_LOCK`]. A poisoned lock (a previous holder panicked)
/// surfaces as a typed error rather than a second panic.
pub(crate) fn pdfium_lock() -> Result<MutexGuard<'static, ()>, CommandError> {
    PDFIUM_LOCK
        .lock()
        .map_err(|_| CommandError::Internal("PDFium lock poisoned".into()))
}

/// Metadata snapshot taken once when the document is opened. The actor
/// caches this so cheap queries (page count, title) don't have to round-
/// trip into `PDFium` on every call.
///
/// Held by both the actor (canonical copy) and `OpenedDocument` (wire
/// copy). Extending this struct is fine; renaming fields is a breaking
/// IPC change — keep the camelCase serde rename stable.
#[derive(Clone, Debug, Default)]
pub struct DocumentMetadata {
    pub page_count: u32,
    pub title: Option<String>,
    pub author: Option<String>,
    pub pdf_version: Option<String>,
}

/// Result of a save. Crosses the IPC boundary as the reply payload of
/// the `pdf_save` command, so the field names are camelCase on the wire.
///
/// `no_op` is `true` only when a same-path save found no unsaved changes
/// and therefore left the user's file untouched (`bytes_written == 0`).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveOutcome {
    pub path: String,
    pub bytes_written: u64,
    pub no_op: bool,
}

/// Shared `PDFium` instance.
///
/// `PDFium` is single-threaded per *document*, but the library itself is
/// safe to share read-only across threads once bound. We bind once via
/// `LazyLock` — `OnceLock` is not enough here because the fallible
/// initializer races: two callers can both call `bind_to_system_library`
/// before either `set`s, and `PDFium`'s global `FPDF_InitLibrary` is not
/// re-entrant (the loser's `Drop` will tear down the library while the
/// winner is still using it → SIGTRAP under `cargo test`'s default
/// parallel runner). `LazyLock` guarantees exactly one init.
///
/// The init result is cached as `Result<Pdfium, String>` because
/// `PdfiumError` isn't `Clone`. Per-call we re-wrap the cached string
/// into a fresh `CommandError`.
static PDFIUM: LazyLock<Result<Pdfium, String>> = LazyLock::new(|| {
    // pdfium-render 0.9 exposes `bind_to_system_library`; the static
    // binding option lives behind a non-default cargo feature we don't
    // depend on. `bind_to_system_library` walks the standard search
    // path (incl. src-tauri/resources/pdfium/ at dev time).
    let bind = || {
        Pdfium::bind_to_system_library()
            .map_err(|e| format!("could not load PDFium: {e}. Run `npm run fetch-pdfium`."))
    };
    // The spare handle has to be taken *first*: once `Pdfium::new` has run,
    // pdfium-render refuses every further bind with
    // `PdfiumLibraryBindingsAlreadyInitialized`. See `RAW_BINDINGS`.
    //
    // The spare must never be dropped: pdfium-render's `Drop` for a binding
    // calls `FPDF_DestroyLibrary`, which would tear the library down under the
    // live instance. Hence `forget` on both paths that could otherwise drop it.
    let spare = bind()?;
    let main = match bind() {
        Ok(main) => main,
        Err(e) => {
            std::mem::forget(spare);
            return Err(e);
        }
    };
    let pdfium = Pdfium::new(main);
    if let Err(unused) = RAW_BINDINGS.set(spare) {
        std::mem::forget(unused); // unreachable: this initialiser runs once
    }
    Ok(pdfium)
});

pub fn pdfium() -> Result<&'static Pdfium, CommandError> {
    PDFIUM.as_ref().map_err(|e| CommandError::Internal(e.clone()))
}

/// What a person sees when an edit is refused on a protected document.
pub const PROTECTED_EDIT_REFUSAL: &str = "This document is password protected, and VibePDF \
    can't make this change to it yet without corrupting the file. Remove the protection with \
    Unlock…, make the change, then protect it again with Protect….";

/// SPEC: P1-VIEW-003 — whether serialised bytes declare encryption: an
/// `/Encrypt` *key with a value* (`/Encrypt 6 0 R` or `/Encrypt <<…>>`).
///
/// A plain substring search is wrong here: an old encryption dictionary left
/// behind as an unreferenced object carries `/EncryptMetadata`, which contains
/// `/Encrypt` and would read as protected.
#[must_use]
pub fn declares_encryption(bytes: &[u8]) -> bool {
    const KEY: &[u8] = b"/Encrypt";
    bytes.windows(KEY.len()).enumerate().any(|(at, window)| {
        window == KEY && {
            let rest = &bytes[at + KEY.len()..];
            let value = rest.iter().position(|b| !b.is_ascii_whitespace()).map(|n| &rest[n..]);
            matches!(value, Some([b'0'..=b'9', ..] | [b'<', b'<', ..]))
        }
    })
}

/// SPEC: P1-VIEW-003 — refuse to edit a protected document through a byte
/// round trip, rather than corrupt it.
///
/// Every edit that goes through lopdf serialises the document, transforms the
/// bytes and reloads them. lopdf cannot encrypt the objects it *adds*: they go
/// out as plaintext inside a file that still declares encryption, and every
/// reader then "decrypts" them into garbage. Measured on 2026-09-13 — a note
/// added to an RC4 permissions-only file saved as `"²Åë\t\u{15}\u{15}/("`, with
/// no error; on a file with an open password the reload failed outright, and
/// passing it the password only produced an empty note plus the note's text in
/// plaintext on disk. Refusing is the honest answer until appended objects can
/// be encrypted with the document's own key.
///
/// `PDFium`-native edits (rotation, cropping) never come through here and keep
/// working on protected documents.
pub fn refuse_if_protected(bytes: &[u8]) -> Result<(), CommandError> {
    if declares_encryption(bytes) {
        return Err(CommandError::InvalidInput(PROTECTED_EDIT_REFUSAL.into()));
    }
    Ok(())
}

/// Whether `PDFium` has a security handler on this document.
///
/// Anything other than a definite "unprotected" counts as protected:
/// pdfium-render only names revisions 2–4, so an AES-256 (R6) document comes
/// back as an unknown revision — which is exactly the case that must refuse.
/// The caller must hold `pdfium_lock`.
pub(crate) fn is_protected(doc: &PdfDocument<'_>) -> bool {
    !matches!(
        doc.permissions().security_handler_revision(),
        Ok(PdfSecurityHandlerRevision::Unprotected)
    )
}

/// Replace the actor's document with the result of a lopdf round trip —
/// unless the document being replaced is protected.
///
/// Checking the *outgoing* document, not the incoming bytes, is the point. For
/// a permissions-only file lopdf can decrypt (RC4 with no open password), lopdf
/// decrypts on load and writes the edit back **without** `/Encrypt`: the bytes
/// look unprotected, and replacing the document with them would make the next
/// save write the file with its protection silently removed. Measured on
/// 2026-09-13 — every edit went through on such a file until this check moved
/// here. The incoming bytes are still checked too, for the files lopdf cannot
/// decrypt, where the edit instead comes back as plaintext inside ciphertext.
/// The caller must hold `pdfium_lock`.
pub(crate) fn replace_with_edited_bytes(
    doc: &mut PdfDocument<'_>,
    bytes: Vec<u8>,
) -> Result<(), CommandError> {
    if is_protected(doc) {
        return Err(CommandError::InvalidInput(PROTECTED_EDIT_REFUSAL.into()));
    }
    *doc = load_edited_bytes(bytes)?;
    Ok(())
}

/// Load bytes back into `PDFium` after a lopdf round trip, refusing protected
/// ones (see [`refuse_if_protected`]). The caller must hold `pdfium_lock`.
///
/// Generic over the lifetime because `PdfDocument` is invariant in it: callers
/// assign the result into an existing `&mut PdfDocument<'a>`.
pub(crate) fn load_edited_bytes<'a>(bytes: Vec<u8>) -> Result<PdfDocument<'a>, CommandError> {
    refuse_if_protected(&bytes)?;
    let engine: &'a Pdfium = pdfium()?;
    engine.load_pdf_from_byte_vec(bytes, None).map_err(CommandError::from)
}

/// A second binding to the same `PDFium` library, for the one call pdfium-render
/// cannot make.
///
/// pdfium-render 0.9.1 hard-codes `flags = 0` in `save_to_writer` (its own
/// TODO names `FPDF_REMOVE_SECURITY`) and keeps the document handle and its
/// bindings `pub(crate)`. The only public way to reach the raw functions is a
/// fresh `bind_to_system_library`, which it permits only before the library is
/// initialised — so `PDFIUM` takes this one first. Both bindings resolve the
/// same loaded library and share its one initialisation. It lives in a static
/// and is never dropped, because dropping a binding destroys the library.
static RAW_BINDINGS: OnceLock<Box<dyn PdfiumLibraryBindings>> = OnceLock::new();

/// `FPDF_REMOVE_SECURITY` from `fpdf_save.h`.
const FPDF_REMOVE_SECURITY: c_ulong = 3;

/// `PDFium`'s `FPDF_FILEWRITE`, with the output buffer riding behind it.
///
/// `PDFium` calls `write_block` with the pointer it was handed. The header is
/// the first field of a `#[repr(C)]` struct, so that pointer is also a pointer
/// to the whole `FileSink` — which is how the callback reaches `buf`.
#[repr(C)]
struct FileSink {
    version: c_int,
    write_block: Option<extern "C" fn(*mut FileSink, *const c_void, c_ulong) -> c_int>,
    buf: Vec<u8>,
}

extern "C" fn append_block(this: *mut FileSink, data: *const c_void, size: c_ulong) -> c_int {
    if size == 0 {
        return 1;
    }
    let Ok(len) = usize::try_from(size) else {
        return 0;
    };
    if this.is_null() || data.is_null() {
        return 0;
    }
    // SAFETY: `this` is the `FileSink` passed to `FPDF_SaveAsCopy`, alive and
    // exclusively borrowed for the whole call; `data` points at `size` bytes
    // `PDFium` owns for the duration of this callback.
    let (sink, block) = unsafe { (&mut *this, std::slice::from_raw_parts(data.cast::<u8>(), len)) };
    sink.buf.extend_from_slice(block);
    1
}

/// SPEC: P1-VIEW-003 — `bytes` re-serialised with their encryption removed,
/// **for the view layer only**.
///
/// PDF.js renders whatever `pdf_get_bytes` returns and has no password, by
/// design: the open password lives only in the prompt's closure
/// (`app/open-with-password.ts`). A normal save keeps `/Encrypt`, so PDF.js
/// raised "No password given" and every encrypted PDF opened to a broken view.
///
/// The result goes over IPC to the webview, which renders and extracts exactly
/// this content anyway. It must never reach a disk write: `save_document`
/// keeps the protection, and `GetBytes` — what Protect, Unlock, Sign and Find
/// operate on — still returns the protected document. `GetViewBytes` is the
/// only caller.
///
/// The caller must hold `pdfium_lock`.
pub(crate) fn remove_security_for_view(bytes: &[u8], password: &str) -> Result<Vec<u8>, CommandError> {
    pdfium()?; // guarantees FPDF_InitLibrary has run and RAW_BINDINGS is set
    let raw = RAW_BINDINGS
        .get()
        .ok_or_else(|| CommandError::Internal("PDFium raw bindings missing".into()))?;

    // SAFETY: `PDFium` does not copy a memory document, so `bytes` must outlive
    // the handle — it does; the handle is closed below, before returning.
    let doc = unsafe { raw.FPDF_LoadMemDocument64(bytes, Some(password)) };
    if doc.is_null() {
        return Err(CommandError::PdfError(
            "could not reopen the document to prepare it for display".into(),
        ));
    }

    let mut sink = FileSink {
        version: 1,
        write_block: Some(append_block),
        buf: Vec::with_capacity(bytes.len()),
    };
    // SAFETY: `sink` is a live, `#[repr(C)]` `FPDF_FILEWRITE` for the whole call.
    let saved = unsafe {
        raw.FPDF_SaveAsCopy(doc, std::ptr::addr_of_mut!(sink).cast(), FPDF_REMOVE_SECURITY)
    };
    // SAFETY: `doc` came from `FPDF_LoadMemDocument64` above and is closed
    // exactly once, on every path past the null check.
    unsafe { raw.FPDF_CloseDocument(doc) };

    if !raw.is_true(saved) {
        return Err(CommandError::PdfError("could not prepare the document for display".into()));
    }
    Ok(sink.buf)
}

/// Open a document and return both the live handle and a metadata
/// snapshot. The returned `PdfDocument<'static>` borrows from the global
/// `Pdfium`, which lives forever, so the lifetime is `'static` for our
/// purposes — the document is owned by the actor thread.
///
/// SPEC: P1-VIEW-001 (open) and P1-VIEW-003 (encrypted PDFs) flow through
/// this entry point. B2 wires the password retry-loop UI; for now any
/// caller may pass `None` and observe a `PdfError` on encrypted files.
pub fn open_pdf<'a>(
    path: &Path,
    password: Option<&str>,
) -> Result<(PdfDocument<'a>, DocumentMetadata), CommandError> {
    let p = pdfium()?;
    // Hold the lock across load + metadata as a single FFI span (calling
    // the unlocked `_inner` so we don't re-lock and deadlock).
    let _guard = pdfium_lock()?;
    let doc = p.load_pdf_from_file(path, password).map_err(CommandError::from)?;
    let metadata = collect_metadata_inner(&doc);
    Ok((doc, metadata))
}

/// Read the metadata fields we surface to the frontend. Errors during
/// metadata extraction are non-fatal — the user can still view the doc.
/// Acquires [`PDFIUM_LOCK`]; on a poisoned lock returns default metadata.
#[must_use]
pub fn collect_metadata(doc: &PdfDocument<'_>) -> DocumentMetadata {
    let Ok(_guard) = pdfium_lock() else {
        return DocumentMetadata::default();
    };
    collect_metadata_inner(doc)
}

/// Body of [`collect_metadata`], assuming [`PDFIUM_LOCK`] is already held
/// by the caller (so `open_pdf` can read metadata under its own guard
/// without re-locking).
#[must_use]
fn collect_metadata_inner(doc: &PdfDocument<'_>) -> DocumentMetadata {
    let page_count = u32::try_from(doc.pages().len()).unwrap_or(u32::MAX);
    let m = doc.metadata();
    let title = m.get(PdfDocumentMetadataTagType::Title).map(|t| t.value().to_string());
    let author = m.get(PdfDocumentMetadataTagType::Author).map(|t| t.value().to_string());
    let pdf_version = Some(format!("{:?}", doc.version()));
    DocumentMetadata {
        page_count,
        title,
        author,
        pdf_version,
    }
}

/// Lightweight metadata-only open used by the smoke test in
/// `tests/pdfium_init.rs` — drops the document immediately after
/// reading the page count. Production callers should go through the
/// actor; see `open_pdf` above.
pub fn open_document_metadata(path: &Path) -> Result<DocumentMetadata, CommandError> {
    let (doc, meta) = open_pdf(path, None)?;
    drop(doc);
    Ok(meta)
}

/// Write the live document to `dest`, atomically and verifiably.
///
/// SPEC: P2-SAVE-001 (proposed) / NFR-PERF-004 — the explicit-save write
/// path. Called only on the document actor thread (`PDFium` is not
/// thread-safe per document); see `pdf::actor`.
///
/// The original file is never destroyed until the new bytes are proven
/// loadable:
///   1. serialize the document and write it to `<name>.vibepdf-tmp` **in
///      `dest`'s own directory** (so the final rename never crosses a
///      filesystem boundary → no `EXDEV`);
///   2. round-trip: re-open that temp file in `PDFium` and confirm it has
///      pages — a write that `PDFium` can't read back is rejected here,
///      before it can clobber anything;
///   3. when `make_backup`, rotate an existing `dest` to `<name>.bak`
///      (one save cycle only — a prior `.bak` is overwritten);
///   4. atomically rename the temp file onto `dest`.
///
/// `password` is the password the document was *opened* with, if any:
/// `PDFium` preserves the source encryption when serializing, so the
/// round-trip verification must unlock the temp file with the same
/// password — otherwise every save of an encrypted document fails
/// `verify_pdf_reopens` with `PasswordRequired` (the P4.HF bug this
/// parameter fixes).
pub fn save_document(
    doc: &PdfDocument<'_>,
    dest: &Path,
    make_backup: bool,
    password: Option<&str>,
) -> Result<SaveOutcome, CommandError> {
    save_document_onto(doc, dest, make_backup, password, None)
}

/// `save_document` for a document that may be signed. When `signed_base` is
/// the file as signed, the edit is appended to it rather than replacing it, so
/// the signatures keep verifying — see `pdf::incremental_save`.
pub fn save_document_onto(
    doc: &PdfDocument<'_>,
    dest: &Path,
    make_backup: bool,
    password: Option<&str>,
    signed_base: Option<&[u8]>,
) -> Result<SaveOutcome, CommandError> {
    let dir = dest.parent().ok_or_else(|| {
        CommandError::InvalidInput(format!("destination has no parent directory: {}", dest.display()))
    })?;
    if !dir.is_dir() {
        return Err(CommandError::NotFound(format!(
            "destination directory does not exist: {}",
            dir.display()
        )));
    }

    // 1. Serialize (under the PDFium lock), then stage to a sibling temp
    //    file. The lock is released before step 2's `verify_pdf_reopens`,
    //    which re-locks via `open_pdf` (the Mutex is not reentrant).
    let bytes = {
        let _guard = pdfium_lock()?;
        doc.save_to_bytes().map_err(CommandError::from)?
    };
    // SPEC: P2-PAGE-003 — clean references to removed pages (dangling links /
    // bookmarks left by a delete or split) before the bytes hit disk. A no-op
    // for documents with nothing dangling; infallible (never breaks a save).
    let bytes = crate::pdf::cos::prune_dangling_destinations(bytes);
    // SPEC: P6-SEC-006 — a signed document's signed bytes stay where they are.
    let bytes = match signed_base {
        Some(base) => crate::pdf::incremental_save::append_as_incremental_update(base, &bytes)?,
        None => bytes,
    };
    let tmp = sibling_with_suffix(dest, ".vibepdf-tmp");
    std::fs::write(&tmp, &bytes)?;

    // 2. Round-trip verification. A bad temp file is cleaned up and the
    //    error surfaced; `dest` is still untouched at this point.
    if let Err(e) = verify_pdf_reopens(&tmp, password) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }

    // 3. Back up the previous version for exactly one save cycle.
    if make_backup && dest.exists() {
        let bak = sibling_with_suffix(dest, ".bak");
        std::fs::rename(dest, &bak)?;
    }

    // 4. Commit.
    std::fs::rename(&tmp, dest)?;

    Ok(SaveOutcome {
        path: dest.to_string_lossy().into_owned(),
        bytes_written: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        no_op: false,
    })
}

/// `foo.pdf` + `.bak` → `foo.pdf.bak` (suffix appended to the *whole*
/// file name, not the stem, so it is unambiguous and reversible).
fn sibling_with_suffix(dest: &Path, suffix: &str) -> PathBuf {
    let mut name = dest
        .file_name()
        .map_or_else(std::ffi::OsString::new, std::ffi::OsStr::to_os_string);
    name.push(suffix);
    dest.with_file_name(name)
}

/// Confirm a freshly-written file re-opens in `PDFium` with at least one
/// page. Runs on the actor thread, which already owns the source
/// document; `PDFium` permits multiple documents open per binding.
/// `password` unlocks the copy when the source document was encrypted
/// (`PDFium` carries the encryption through the save).
fn verify_pdf_reopens(path: &Path, password: Option<&str>) -> Result<(), CommandError> {
    let (doc, meta) = open_pdf(path, password)?;
    let pages = meta.page_count;
    drop(doc);
    if pages == 0 {
        return Err(CommandError::PdfError(
            "saved file re-opened with zero pages".into(),
        ));
    }
    Ok(())
}

#[must_use]
pub fn pdfium_version_string() -> String {
    // pdfium-render doesn't expose a runtime version directly; the
    // version is determined by the loaded native lib. For the smoke
    // test we just confirm we can bind.
    match pdfium() {
        Ok(_) => "pdfium: loaded".to_string(),
        Err(e) => format!("pdfium: failed — {e}"),
    }
}

#[cfg(test)]
mod declares_encryption_tests {
    use super::declares_encryption;

    #[test]
    fn a_trailer_reference_or_inline_dictionary_is_protection() {
        assert!(declares_encryption(b"trailer\n<</Root 1 0 R/Encrypt 6 0 R/Size 7>>"));
        assert!(declares_encryption(b"trailer <</Encrypt\r\n  12 0 R>>"));
        assert!(declares_encryption(b"<</Encrypt<</Filter/Standard/V 5>>>>"));
    }

    // PDFium's REMOVE_SECURITY leaves the old dictionary behind as an orphan,
    // and its key contains the substring. That is not protection.
    #[test]
    fn encrypt_metadata_and_plain_files_are_not() {
        assert!(!declares_encryption(b"6 0 obj <</EncryptMetadata true/Filter/Standard>> endobj trailer <</Root 1 0 R>>"));
        assert!(!declares_encryption(b"%PDF-1.4 1 0 obj <</Type/Catalog>> endobj trailer <</Root 1 0 R>>"));
    }
}

