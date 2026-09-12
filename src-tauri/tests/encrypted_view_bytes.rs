//! SPEC: P1-VIEW-003 — an encrypted document must be *viewable*, not merely
//! openable.
//!
//! `encrypted_open.rs` asserts the actor opens the file and reports a page
//! count. That is the backend. The view layer renders whatever `pdf_get_bytes`
//! returns through PDF.js, which has no password and must not be given one —
//! the open password lives only in the prompt (`app/open-with-password.ts`).
//! Those bytes used to keep `/Encrypt`, PDF.js raised "No password given", and
//! every encrypted PDF opened to a broken view. Found by the P6 sweep.
//!
//! The other half matters as much. The decrypted bytes are for the screen only:
//! saving, and every command that operates on the document's bytes (Protect,
//! Unlock, Sign, Find), must still see the protected document.

use std::path::{Path, PathBuf};

use vibepdf_lib::error::CommandError;
use vibepdf_lib::pdf::actor::DocumentActorHandle;
use vibepdf_lib::security::encrypt::{encrypt_document, DocumentPermissions, EncryptOptions};

const PASSWORD: &str = "view-me";

struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("vibepdf-view-bytes-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Self(dir)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn hello() -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/basic/hello.pdf"))
        .expect("hello.pdf")
}

/// `hello.pdf`, protected the way the Protect command protects it.
fn protected_hello(scratch: &Scratch) -> PathBuf {
    let opts = EncryptOptions {
        user_password: Some(PASSWORD.into()),
        owner_password: None,
        permissions: DocumentPermissions::default(),
    };
    let path = scratch.path("protected.pdf");
    std::fs::write(&path, encrypt_document(&hello(), &opts).expect("encrypt")).expect("write");
    path
}

/// Whether the file declares encryption: an `/Encrypt` *key with a value*.
///
/// Not a substring search. `FPDF_REMOVE_SECURITY` drops the trailer's reference
/// but still writes the old encryption dictionary as an unreferenced object, and
/// its `/EncryptMetadata` key contains `/Encrypt` — so a plain search reports a
/// decrypted file as protected. That exact false alarm is how this was written.
fn declares_encryption(bytes: &[u8]) -> bool {
    let key = b"/Encrypt";
    bytes.windows(key.len()).enumerate().any(|(i, w)| {
        if w != key {
            return false;
        }
        let rest = &bytes[i + key.len()..];
        let value = rest.iter().position(|b| !b.is_ascii_whitespace()).map(|n| &rest[n..]);
        // `/Encrypt 6 0 R` or an inline `/Encrypt <<…>>` — never `/EncryptMetadata`.
        matches!(value, Some([b'0'..=b'9', ..]) | Some([b'<', b'<', ..]))
    })
}

/// `bytes` with the trailer `/ID` removed. `PDFium` writes a fresh one on every
/// save, so two serialisations of an unchanged document differ only there.
fn without_file_id(bytes: &[u8]) -> Vec<u8> {
    let key = b"/ID";
    match bytes.windows(key.len()).rposition(|w| w == key) {
        Some(start) => {
            let end = bytes[start..].iter().position(|&b| b == b']').map_or(bytes.len(), |n| start + n + 1);
            [&bytes[..start], &bytes[end..]].concat()
        }
        None => bytes.to_vec(),
    }
}

/// What a reader with no password extracts from page 1.
fn text_without_password(bytes: &[u8]) -> String {
    let doc = lopdf::Document::load_mem(bytes).expect("loads without a password");
    assert!(!doc.is_encrypted(), "still encrypted");
    doc.extract_text(&[1]).expect("extract text")
}

fn open(path: PathBuf, password: Option<&str>) -> Result<DocumentActorHandle, CommandError> {
    DocumentActorHandle::spawn(None, uuid::Uuid::new_v4(), path, password.map(str::to_string))
}

#[tokio::test]
async fn view_bytes_of_a_protected_document_need_no_password() {
    let scratch = Scratch::new("view");
    let handle = open(protected_hello(&scratch), Some(PASSWORD)).expect("opens with its password");
    let view = handle.get_view_bytes().await.expect("view bytes");

    assert!(!declares_encryption(&view), "the view layer would need a password it does not have");
    // Assert on the content, not the dictionary: an /Encrypt stripped from bytes
    // whose streams are still ciphertext passes the line above and renders blank.
    assert!(text_without_password(&view).contains("Hello, VibePDF."));

    // Our own engine agrees: the bytes open with no password at all.
    let reopened = scratch.path("view.pdf");
    std::fs::write(&reopened, &view).expect("write view bytes");
    assert_eq!(open(reopened, None).expect("opens without a password").metadata().page_count, 1);
}

#[tokio::test]
async fn the_view_shows_unsaved_edits_to_a_protected_document() {
    let scratch = Scratch::new("edited");
    let handle = open(protected_hello(&scratch), Some(PASSWORD)).expect("opens");
    handle.rotate_pages(vec![0], 1).await.expect("rotate");

    let view = handle.get_view_bytes().await.expect("view bytes");
    assert!(!declares_encryption(&view));
    let doc = lopdf::Document::load_mem(&view).expect("load");
    let page = *doc.get_pages().values().next().expect("a page");
    let rotate = doc
        .get_dictionary(page)
        .expect("page dictionary")
        .get(b"Rotate")
        .and_then(lopdf::Object::as_i64)
        .unwrap_or(0);
    assert_eq!(rotate, 90, "the view must show the in-memory rotation, not the file on disk");
}

// The guard. Decrypted bytes exist for one purpose, and a regression that let
// them reach a command or a save would silently strip the protection from the
// user's file — the worst failure available in this module, and one that looks
// like success.
#[tokio::test]
async fn a_protected_document_stays_protected_everywhere_but_the_screen() {
    let scratch = Scratch::new("guard");
    let handle = open(protected_hello(&scratch), Some(PASSWORD)).expect("opens");
    let _ = handle.get_view_bytes().await.expect("view bytes"); // exercise the view path first

    let bytes = handle.get_bytes().await.expect("bytes");
    assert!(declares_encryption(&bytes), "Protect, Unlock, Sign and Find must see the protected document");

    let dest = scratch.path("saved.pdf");
    handle.save(Some(dest.clone())).await.expect("save");
    assert!(
        declares_encryption(&std::fs::read(&dest).expect("read saved")),
        "saving must never write the decrypted view bytes"
    );
    assert!(
        matches!(open(dest, None).err(), Some(CommandError::PasswordRequired(_))),
        "a saved protected document opened without its password"
    );
}

#[tokio::test]
async fn an_unprotected_document_is_served_as_is() {
    let scratch = Scratch::new("plain");
    let path = scratch.path("hello.pdf");
    std::fs::write(&path, hello()).expect("write");
    let handle = open(path, None).expect("opens");
    assert_eq!(
        without_file_id(&handle.get_view_bytes().await.expect("view bytes")),
        without_file_id(&handle.get_bytes().await.expect("bytes")),
        "an unprotected document must reach the view exactly as the commands see it"
    );
}

#[tokio::test]
async fn a_document_protected_by_another_tool_is_viewable() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/acceptance/p1-encrypted.pdf");
    if !fixture.exists() {
        eprintln!("skipping: regenerate with `python3 tests/fixtures/acceptance/generate.py`");
        return;
    }
    let handle = open(fixture, Some("vibepdf")).expect("opens with its password");
    let view = handle.get_view_bytes().await.expect("view bytes");
    assert!(!declares_encryption(&view));
    assert!(text_without_password(&view).contains("Hello, VibePDF."));
}
