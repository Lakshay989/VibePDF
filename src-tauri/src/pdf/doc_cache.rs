//! A per-actor cache of the live document's parsed `lopdf::Document`.
//!
//! SPEC: NFR-PERF-005 (proposed) — every annotation read used to serialize the
//! `PDFium` document and re-parse the whole thing with lopdf (`Document::load_mem`,
//! seconds on a large file). A post-edit epoch bump fires a *burst* of such reads
//! (annotations panel, text boxes, free-text, notes, measure — across every
//! visible page), so the single actor thread re-parsed the document many times
//! over, and the panel + thumbnails lagged.
//!
//! This caches the parse: the first read after an edit parses once; the rest of
//! the burst reuse it. A write [`invalidate`](CachedDoc::invalidate)s the cache,
//! so the next read re-parses the *edited* bytes — no drift, because the cache is
//! never mutated in place (this stage only speeds reads; in-place write mutation
//! is a later increment). Parsing is lazy and driven by a closure, so a warm
//! cache costs nothing — no serialize, no `PDFium` lock.

use lopdf::Document;

use crate::error::CommandError;

/// Lazily-parsed, invalidate-on-write cache of the actor's `lopdf::Document`.
#[derive(Default)]
pub struct CachedDoc {
    doc: Option<Document>,
}

impl CachedDoc {
    /// An empty (cold) cache.
    #[must_use]
    pub fn new() -> Self {
        Self { doc: None }
    }

    /// Drop the cached parse; the next [`get`](Self::get) re-parses. Call after
    /// any operation that changes the live document (edits, undo/redo, restore).
    pub fn invalidate(&mut self) {
        self.doc = None;
    }

    /// Borrow the parsed document, parsing bytes from `produce` once on a cold
    /// cache. `produce` runs only on a miss, so a warm cache does no work.
    pub fn get<F>(&mut self, produce: F) -> Result<&Document, CommandError>
    where
        F: FnOnce() -> Result<Vec<u8>, CommandError>,
    {
        if self.doc.is_none() {
            let bytes = produce()?;
            let parsed = Document::load_mem(&bytes)
                .map_err(|e| CommandError::Internal(format!("lopdf parse (doc cache): {e}")))?;
            self.doc = Some(parsed);
        }
        // Filled immediately above when `None`, so this is always `Some`; the
        // fallback keeps us off `unwrap` (clippy::pedantic / no-unwrap rule).
        self.doc
            .as_ref()
            .ok_or_else(|| CommandError::Internal("doc cache empty after fill".into()))
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::CachedDoc;

    // Minimal one-page PDF, enough for `Document::load_mem`.
    const MINI_PDF: &[u8] = b"%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>endobj\nxref\n0 4\n0000000000 65535 f \n0000000009 00000 n \n0000000052 00000 n \n0000000101 00000 n \ntrailer<</Size 4/Root 1 0 R>>\nstartxref\n164\n%%EOF";

    #[test]
    fn parses_once_then_reuses_until_invalidated() {
        use std::cell::Cell;
        let mut cache = CachedDoc::new();
        // A `Cell` counter (interior mutability) so each fresh producer closure
        // borrows it immutably and we can read the count between calls.
        let parses = Cell::new(0u32);
        let produce = || {
            parses.set(parses.get() + 1);
            Ok::<_, crate::error::CommandError>(MINI_PDF.to_vec())
        };

        // Two reads on a cold-then-warm cache: only one parse.
        cache.get(produce).expect("parse 1");
        cache.get(produce).expect("reuse");
        assert_eq!(parses.get(), 1, "warm cache must not re-parse");

        // Invalidate → the next read parses again.
        cache.invalidate();
        cache.get(produce).expect("parse 2");
        assert_eq!(parses.get(), 2, "invalidate must force a re-parse");
    }
}
