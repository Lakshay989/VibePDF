//! Split a PDF into multiple files (P2.C3).
//!
//! SPEC: P2-PAGE-007 — four modes: (a) every N pages, (b) at specific page
//! numbers, (c) by file-size target, (d) by top-level bookmarks. Each mode
//! reduces to a list of *contiguous* 0-based page groups; every group becomes
//! one output file written through the same verified subset-writer as extract
//! (P2.C2), so each file is atomic + round-trip-checked. Read-only on the
//! source: no mutation, no undo, no dirty.

use std::path::Path;

use pdfium_render::prelude::PdfDocument;
use serde::{Deserialize, Serialize};

use crate::error::CommandError;
use crate::pdf::document::{pdfium, pdfium_lock, SaveOutcome};
use crate::pdf::extract::write_subset_pdf;

/// How to divide the document. Crosses IPC as `{ "kind": "...", ... }`
/// (kebab/camel tag per `docs/06_CONVENTIONS.md`).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SplitMode {
    /// (a) Fixed run length: every `n` pages becomes one file.
    EveryN { n: u32 },
    /// (b) Split *before* each 1-based page number in `boundaries`.
    AtPages { boundaries: Vec<i32> },
    /// (c) Grow a file until adding the next page would exceed `max_bytes`.
    BySize {
        #[serde(rename = "maxBytes")]
        max_bytes: u64,
    },
    /// (d) Start a new file at every top-level bookmark's target page.
    Bookmarks,
}

/// The set of files produced by a split — one `SaveOutcome` per output file,
/// in page order. Crosses IPC as the reply of `pdf_split_document`.
#[derive(Debug, Clone, Serialize)]
pub struct SplitOutcome {
    pub files: Vec<SaveOutcome>,
}

/// Split `source` into multiple PDFs under `dest_dir`, named
/// `{stem}-001.pdf`, `{stem}-002.pdf`, … Each output file is written
/// atomically and round-trip-verified.
///
/// SPEC: P2-PAGE-007.
pub fn split_document(
    source: &PdfDocument<'_>,
    mode: &SplitMode,
    dest_dir: &Path,
    stem: &str,
) -> Result<SplitOutcome, CommandError> {
    let groups = plan_groups(source, mode)?;
    if groups.len() < 2 {
        return Err(CommandError::InvalidInput(
            "split would produce fewer than two files".into(),
        ));
    }

    let mut files = Vec::with_capacity(groups.len());
    for (i, indices) in groups.iter().enumerate() {
        // 1-based, zero-padded so a 10+ file split sorts lexically.
        let dest = dest_dir.join(format!("{stem}-{:03}.pdf", i + 1));
        files.push(write_subset_pdf(source, indices, &dest)?);
    }
    Ok(SplitOutcome { files })
}

/// The live page count, read under the `PDFium` lock.
fn page_count(source: &PdfDocument<'_>) -> Result<i32, CommandError> {
    let _guard = pdfium_lock()?;
    Ok(source.pages().len())
}

/// Compute contiguous 0-based page groups for `mode`. Returns a typed error
/// *before* any file is written, so a bad request can't leave partial output.
fn plan_groups(
    source: &PdfDocument<'_>,
    mode: &SplitMode,
) -> Result<Vec<Vec<i32>>, CommandError> {
    let count = page_count(source)?;
    if count <= 0 {
        return Err(CommandError::InvalidInput("document has no pages".into()));
    }

    match mode {
        SplitMode::EveryN { n } => groups_every_n(count, *n),
        SplitMode::AtPages { boundaries } => groups_at_pages(count, boundaries),
        SplitMode::BySize { max_bytes } => groups_by_size(source, count, *max_bytes),
        SplitMode::Bookmarks => groups_by_bookmarks(source, count),
    }
}

/// Build contiguous groups from sorted, in-range *start* indices. The first
/// group always starts at 0 (a leading start of 0 is implied).
fn groups_from_starts(count: i32, starts: &[i32]) -> Vec<Vec<i32>> {
    // `starts` is sorted, de-duped, every entry in `1..count` (0 is implicit).
    let mut bounds = Vec::with_capacity(starts.len() + 2);
    bounds.push(0);
    bounds.extend(starts.iter().copied());
    bounds.push(count);

    bounds
        .windows(2)
        .map(|w| (w[0]..w[1]).collect())
        .collect()
}

/// (a) Every `n` pages.
fn groups_every_n(count: i32, n: u32) -> Result<Vec<Vec<i32>>, CommandError> {
    if n == 0 {
        return Err(CommandError::InvalidInput("pages per file must be ≥ 1".into()));
    }
    let step = usize::try_from(n).unwrap_or(usize::MAX);
    let n = i32::try_from(n).unwrap_or(i32::MAX);
    let starts: Vec<i32> = (n..count).step_by(step).collect();
    Ok(groups_from_starts(count, &starts))
}

/// (b) Split before each 1-based page number.
fn groups_at_pages(count: i32, boundaries: &[i32]) -> Result<Vec<Vec<i32>>, CommandError> {
    if boundaries.is_empty() {
        return Err(CommandError::InvalidInput("no split points specified".into()));
    }
    let mut starts = Vec::with_capacity(boundaries.len());
    for &b in boundaries {
        // 1-based page number; a split *before* page 1 (or beyond the last
        // page) is meaningless.
        if b <= 1 || b > count {
            return Err(CommandError::InvalidInput(format!(
                "split point out of range: {b} (document has {count} pages)"
            )));
        }
        starts.push(b - 1); // 1-based page → 0-based start index
    }
    starts.sort_unstable();
    starts.dedup();
    Ok(groups_from_starts(count, &starts))
}

/// (c) Greedy by file-size target. Grows a chunk one page at a time,
/// serializing to measure bytes; closes the chunk when adding the next page
/// would exceed `max_bytes` (a lone page bigger than the target still gets
/// its own file — no page is ever dropped). Approximate by nature: `PDFium`
/// offers no size oracle and shared resources compress unpredictably.
fn groups_by_size(
    source: &PdfDocument<'_>,
    count: i32,
    max_bytes: u64,
) -> Result<Vec<Vec<i32>>, CommandError> {
    if max_bytes == 0 {
        return Err(CommandError::InvalidInput(
            "size target must be greater than zero".into(),
        ));
    }

    let mut groups: Vec<Vec<i32>> = Vec::new();
    let mut current: Vec<i32> = Vec::new();
    for page in 0..count {
        let mut candidate = current.clone();
        candidate.push(page);
        let size = probe_chunk_size(source, &candidate)?;
        if size > max_bytes && !current.is_empty() {
            // The page won't fit; flush what we have and start fresh.
            groups.push(std::mem::take(&mut current));
            current.push(page);
        } else {
            current.push(page);
        }
    }
    if !current.is_empty() {
        groups.push(current);
    }
    Ok(groups)
}

/// Serialize a candidate chunk and return its byte length, without writing
/// a file. Used only by the by-size planner.
fn probe_chunk_size(source: &PdfDocument<'_>, indices: &[i32]) -> Result<u64, CommandError> {
    use crate::pdf::delete_page::range_string;

    let _guard = pdfium_lock()?;
    let pdfium = pdfium()?;
    let mut out = pdfium.create_new_pdf().map_err(CommandError::from)?;
    out.pages_mut()
        .copy_pages_from_document(source, &range_string(indices), 0)
        .map_err(CommandError::from)?;
    let bytes = out.save_to_bytes().map_err(CommandError::from)?;
    Ok(bytes.len() as u64)
}

/// (d) Start a new file at every top-level bookmark's destination page.
/// Read-only outline access — no reference rewriting, so no `lopdf` needed.
fn groups_by_bookmarks(
    source: &PdfDocument<'_>,
    count: i32,
) -> Result<Vec<Vec<i32>>, CommandError> {
    let starts = {
        let _guard = pdfium_lock()?;
        let bookmarks = source.bookmarks();
        let mut starts: Vec<i32> = Vec::new();
        // `root()` is the *first* top-level bookmark (its parent is the
        // synthetic outline root); walk siblings to visit them all.
        let mut node = bookmarks.root();
        while let Some(b) = node {
            if let Some(idx) = b.destination().and_then(|d| d.page_index().ok()) {
                if idx > 0 && idx < count {
                    // Page 0 is the implicit first start; skip it as a boundary.
                    starts.push(idx);
                }
            }
            node = b.next_sibling();
        }
        starts.sort_unstable();
        starts.dedup();
        starts
    };

    if starts.is_empty() {
        return Err(CommandError::InvalidInput(
            "no top-level bookmarks to split on".into(),
        ));
    }
    Ok(groups_from_starts(count, &starts))
}
