//! The local signature library (P6.A1) — the store A2–A4 write into.
//!
//! Infrastructure, not a spec line of its own: `P6-SEC-001` names "the local
//! signature library" as where a drawn signature is saved, and -002 / -003 add
//! typed and image signatures to the same place. This module is that place.
//!
//! **It lives in `settings/`, not `security/`, deliberately.** It stores PNG
//! bytes the user drew and a timestamp — no keys, no certificates, no signing.
//! `security/` carries a per-change human-review rule (`steps/P6.md`) that is
//! there for the PKCS#7 work in B1; extending it to a PNG store would dilute the
//! rule where it actually matters without making anything safer.
//!
//! Same two-layer split as [`crate::settings::recents`]: pure list logic with no
//! IO, then disk IO against an explicit `&Path`. Neither layer knows about
//! `AppHandle` — that lives in `commands/signatures.rs`.
//!
//! ## On-disk shape
//!
//! ```text
//! <app_data_dir>/signatures/
//!   index.json          versioned metadata: id, kind, created_at
//!   <id>.png            one blob per entry
//! ```
//!
//! The index stays small (no base64 blobs inline), and a corrupt blob fails
//! only its own entry instead of taking the whole library with it.
//!
//! Write order is **blob first, then index**. A crash between the two leaves an
//! orphaned blob, which is invisible and harmless; the reverse would leave an
//! index row pointing at a file that does not exist.
//!
//! Defensive posture inherited from `recents`: a missing or corrupt `index.json`
//! reads as an empty library rather than erroring. A signature library is a
//! convenience — never a reason to block the app.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::CommandError;
use crate::settings::{read_json, write_atomic};

/// Maximum stored signatures; the oldest are pruned past this. Matches
/// `recents::MAX_RECENTS` — a user curating more than twenty signatures is
/// managing a collection, which is not what this is for.
pub const MAX_SIGNATURES: usize = 20;

/// How a signature was produced. The bytes are always PNG regardless — the kind
/// records provenance so the UI can group and re-edit appropriately (A2–A4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SignatureKind {
    /// Drawn with a pointer (P6-SEC-001).
    Draw,
    /// Typed and rendered in a handwriting font (P6-SEC-002).
    Type,
    /// Imported from an image file (P6-SEC-003).
    Image,
}

impl SignatureKind {
    /// Parse the wire string. Unknown values are rejected rather than defaulted:
    /// a typo would otherwise silently file a signature under the wrong kind.
    pub fn parse(s: &str) -> Result<Self, CommandError> {
        match s {
            "draw" => Ok(Self::Draw),
            "type" => Ok(Self::Type),
            "image" => Ok(Self::Image),
            other => Err(CommandError::InvalidInput(format!("unknown signature kind: {other}"))),
        }
    }
}

/// One library entry. The bytes live beside it in `<id>.png`, not in here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignatureEntry {
    /// Opaque id, and the blob's filename stem.
    pub id: String,
    pub kind: SignatureKind,
    /// Unix milliseconds. Ordering key — the list is newest-first.
    pub created_at: u64,
}

/// On-disk index. `version` is carried for forward-compat, so a future format
/// change can branch on it instead of silently mis-parsing.
#[derive(Debug, Default, Serialize, Deserialize)]
struct IndexFile {
    version: u32,
    entries: Vec<SignatureEntry>,
}

const CURRENT_VERSION: u32 = 1;

/// PNG's 8-byte file signature. The only format we store — A4 converts JPG/BMP
/// on the way in, so anything else here is a caller bug worth failing loudly.
const PNG_MAGIC: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// `<dir>/index.json`.
#[must_use]
pub fn index_path(dir: &Path) -> PathBuf {
    dir.join("index.json")
}

/// `<dir>/<id>.png`.
#[must_use]
pub fn blob_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.png"))
}

// ── pure list logic (no IO) ─────────────────────────────────────────────────

/// Insert `entry` at the front and truncate to [`MAX_SIGNATURES`], returning the
/// ids that fell off the end so the caller can delete their blobs.
///
/// Pure: no IO. The caller owns blob cleanup, because this layer does not know
/// where blobs live.
pub fn push_front(list: &mut Vec<SignatureEntry>, entry: SignatureEntry) -> Vec<String> {
    list.insert(0, entry);
    if list.len() <= MAX_SIGNATURES {
        return Vec::new();
    }
    list.drain(MAX_SIGNATURES..).map(|e| e.id).collect()
}

/// Drop the entry with `id`. Returns whether anything was removed — a caller
/// removing an id that is already gone is a no-op, not an error.
pub fn remove_by_id(list: &mut Vec<SignatureEntry>, id: &str) -> bool {
    let before = list.len();
    list.retain(|e| e.id != id);
    before != list.len()
}

// ── disk IO ─────────────────────────────────────────────────────────────────

/// Read the library index from `dir`. Missing or corrupt → empty, per the
/// module docs.
#[must_use]
pub fn load(dir: &Path) -> Vec<SignatureEntry> {
    match read_json::<IndexFile>(&index_path(dir)) {
        Some(parsed) if parsed.version == CURRENT_VERSION => {
            let mut entries = parsed.entries;
            // Defend against a hand-edited index that exceeds the cap.
            entries.truncate(MAX_SIGNATURES);
            entries
        }
        _ => Vec::new(),
    }
}

/// Atomically persist the index. See `settings::write_atomic`.
pub fn save(dir: &Path, entries: &[SignatureEntry]) -> Result<(), CommandError> {
    let file = IndexFile { version: CURRENT_VERSION, entries: entries.to_vec() };
    let bytes = serde_json::to_vec_pretty(&file)
        .map_err(|e| CommandError::Internal(format!("encode signature index: {e}")))?;
    write_atomic(&index_path(dir), &bytes)
}

/// Store `png` as a new entry and return it.
///
/// Blob first, then index — see the module docs on crash ordering. Pruned
/// entries' blobs are deleted on a best-effort basis: a failed unlink leaves a
/// stray file, which costs disk but cannot corrupt the library.
pub fn add(
    dir: &Path,
    kind: SignatureKind,
    png: &[u8],
    created_at: u64,
) -> Result<SignatureEntry, CommandError> {
    if !png.starts_with(&PNG_MAGIC) {
        return Err(CommandError::InvalidInput("signature bytes are not a PNG".into()));
    }
    let entry =
        SignatureEntry { id: uuid::Uuid::new_v4().to_string(), kind, created_at };

    std::fs::create_dir_all(dir)?;
    write_atomic(&blob_path(dir, &entry.id), png)?;

    let mut list = load(dir);
    let pruned = push_front(&mut list, entry.clone());
    save(dir, &list)?;
    for id in pruned {
        let _ = std::fs::remove_file(blob_path(dir, &id));
    }
    Ok(entry)
}

/// Delete `id` from the index and unlink its blob. Removing an unknown id
/// succeeds — the end state the caller asked for is already true.
pub fn remove(dir: &Path, id: &str) -> Result<(), CommandError> {
    let mut list = load(dir);
    if remove_by_id(&mut list, id) {
        save(dir, &list)?;
    }
    let _ = std::fs::remove_file(blob_path(dir, id));
    Ok(())
}

/// The PNG bytes for `id`. A missing or unreadable blob is `NotFound` rather
/// than a panic — one damaged blob must not take down the library.
pub fn bytes(dir: &Path, id: &str) -> Result<Vec<u8>, CommandError> {
    std::fs::read(blob_path(dir, id))
        .map_err(|_| CommandError::NotFound(format!("signature blob: {id}")))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::{push_front, remove_by_id, SignatureEntry, SignatureKind, MAX_SIGNATURES};

    fn entry(id: &str, at: u64) -> SignatureEntry {
        SignatureEntry { id: id.to_owned(), kind: SignatureKind::Draw, created_at: at }
    }

    #[test]
    fn push_front_puts_the_newest_first() {
        let mut list = vec![entry("a", 1)];
        let pruned = push_front(&mut list, entry("b", 2));
        assert_eq!(list.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["b", "a"]);
        assert!(pruned.is_empty());
    }

    #[test]
    fn push_front_prunes_past_the_cap_and_names_the_casualties() {
        let mut list: Vec<SignatureEntry> =
            (0..MAX_SIGNATURES).map(|i| entry(&format!("old{i}"), i as u64)).collect();
        let pruned = push_front(&mut list, entry("new", 999));
        assert_eq!(list.len(), MAX_SIGNATURES);
        assert_eq!(list[0].id, "new");
        // The caller needs the id to unlink the blob.
        assert_eq!(pruned, vec![format!("old{}", MAX_SIGNATURES - 1)]);
    }

    #[test]
    fn remove_by_id_reports_whether_it_did_anything() {
        let mut list = vec![entry("a", 1), entry("b", 2)];
        assert!(remove_by_id(&mut list, "a"));
        assert_eq!(list.len(), 1);
        // Already gone — a no-op, and it says so.
        assert!(!remove_by_id(&mut list, "a"));
    }

    #[test]
    fn kind_round_trips_and_rejects_typos() {
        for (s, k) in [
            ("draw", SignatureKind::Draw),
            ("type", SignatureKind::Type),
            ("image", SignatureKind::Image),
        ] {
            assert_eq!(SignatureKind::parse(s).unwrap(), k);
        }
        assert!(SignatureKind::parse("drawn").is_err());
    }
}
